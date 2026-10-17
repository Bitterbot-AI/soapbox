use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use soapbox::{
    api::{self, App},
    config::Principal,
    model::digest,
    store::Store,
    worker::ApiClient,
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const BUYER: &str = "buyer-token-for-test-only-000000000000";
const WORKER: &str = "worker-token-for-test-only-00000000000";
const WORKER2: &str = "worker-two-test-only-00000000000000000";
fn principal(id: &str, roles: &[&str]) -> Principal {
    Principal {
        id: id.into(),
        token_env: "UNUSED".into(),
        roles: roles.iter().map(|s| s.to_string()).collect(),
        pools: vec!["circle".into()],
        projects: vec!["science".into()],
    }
}
fn app(path: &std::path::Path) -> App {
    App {
        store: Arc::new(Mutex::new(Store::open(path, 100, 4096).unwrap())),
        identities: Arc::new(vec![
            (
                digest(BUYER.as_bytes()),
                principal("buyer", &["admin", "submitter"]),
            ),
            (digest(WORKER.as_bytes()), principal("worker", &["worker"])),
            (
                digest(WORKER2.as_bytes()),
                principal("worker2", &["worker"]),
            ),
        ]),
    }
}
#[tokio::test]
async fn http_auth_role_and_origin_gates() {
    let router = api::router(app(std::path::Path::new(":memory:")));
    for (path, token, origin, status) in [
        ("/v1/status", None, None, 401),
        (
            "/v1/status",
            Some(BUYER),
            Some("https://untrusted.example"),
            403,
        ),
        ("/v1/jobs", Some(WORKER), None, 403),
    ] {
        let mut req = Request::builder()
            .uri(path)
            .method(if path == "/v1/jobs" { "POST" } else { "GET" })
            .header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        let body = if path == "/v1/jobs" {
            json!({"idempotency_key":"bad","mode":{"kind":"private","pool_id":"circle"},"workload":{"kind":"sha256","texts":["x"]}}).to_string()
        } else {
            String::new()
        };
        assert_eq!(
            router
                .clone()
                .oneshot(req.body(Body::from(body)).unwrap())
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
}
#[test]
fn transport_never_allows_plaintext_remote_credentials() {
    assert!(ApiClient::new("http://example.com", BUYER.into()).is_err());
    assert!(ApiClient::new("https://user:pass@example.com", BUYER.into()).is_err());
    assert!(ApiClient::new("http://127.0.0.1:8787", BUYER.into()).is_ok());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_worker_processes_execute_all_modes_and_store_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let state = app(&dir.path().join("state.sqlite"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let server =
        tokio::spawn(async move { axum::serve(listener, api::router(state)).await.unwrap() });
    let client = ApiClient::new(&address, BUYER.into()).unwrap();
    client
        .post::<Value>(
            "/v1/credits",
            json!({"account":"buyer","amount":10,"idempotency_key":"seed"}),
        )
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (i, mode) in [
        json!({"kind":"paid","max_cost_credits":2}),
        json!({"kind":"private","pool_id":"circle"}),
        json!({"kind":"collective","project_id":"science"}),
    ]
    .into_iter()
    .enumerate()
    {
        let j:Value=client.post("/v1/jobs",json!({"idempotency_key":format!("job-{i}"),"mode":mode,"workload":{"kind":"sha256","texts":["one","two"]}})).await.unwrap();
        ids.push(j["id"].as_str().unwrap().to_string());
    }
    let config = dir.path().join("worker.json");
    std::fs::write(&config,json!({"allow_unsafe_host":true,"modes":["paid","private","collective"],"pools":["circle"],"projects":["science"],"memory_mib":256}).to_string()).unwrap();
    for token in [WORKER, WORKER2, WORKER] {
        let status = tokio::process::Command::new(env!("CARGO_BIN_EXE_soapbox"))
            .args(["worker", "--config"])
            .arg(&config)
            .args(["--coordinator", &address, "--once"])
            .env("SOAPBOX_WORKER_TOKEN", token)
            .kill_on_drop(true)
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }
    for id in ids {
        let j: Value = client
            .request(reqwest::Method::GET, &format!("/v1/jobs/{id}"), None)
            .await
            .unwrap();
        assert_eq!(j["status"], "succeeded");
        assert_eq!(j["receipt"]["verification"], "recomputed");
    }
    let snapshot: Value = client
        .request(reqwest::Method::GET, "/v1/status", None)
        .await
        .unwrap();
    assert_eq!(snapshot["account"]["available_credits"], 8);
    assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 2);
    let operator = ApiClient::new(&address, WORKER2.into()).unwrap();
    let own: Vec<Value> = operator
        .request(reqwest::Method::GET, "/v1/jobs", None)
        .await
        .unwrap();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["worker"], "worker2");
    let other = snapshot["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["worker"] == "worker")
        .unwrap();
    assert!(operator
        .request::<Value>(
            reqwest::Method::GET,
            &format!("/v1/jobs/{}", other["id"].as_str().unwrap()),
            None
        )
        .await
        .is_err());
    let own_id = own[0]["id"].as_str().unwrap();
    assert!(operator
        .request::<Value>(reqwest::Method::GET, &format!("/v1/jobs/{own_id}"), None)
        .await
        .is_ok());
    assert!(operator
        .post::<Value>("/v1/workers/worker/pause", json!({"paused":true}))
        .await
        .is_err());
    assert!(operator
        .post::<Value>("/v1/workers/worker2/pause", json!({"paused":true}))
        .await
        .is_ok());
    assert!(operator
        .request::<Value>(reqwest::Method::GET, "/v1/settlements", None)
        .await
        .is_err());

    let stored: Value = client
        .post(
            "/v1/artifacts",
            json!({"content":"a shared result","ttl_seconds":60}),
        )
        .await
        .unwrap();
    let retrieved: Value = client
        .request(
            reqwest::Method::GET,
            &format!("/v1/artifacts/{}", stored["id"].as_str().unwrap()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(retrieved["content"], "a shared result");
    server.abort();
}
#[tokio::test]
async fn error_responses_classify_access_without_disclosing_internal_data() {
    use http_body_util::BodyExt;
    let state = app(std::path::Path::new(":memory:"));
    let id = state.store.lock().unwrap().submit(
        &principal("buyer", &["submitter"]),
        serde_json::from_value(json!({"idempotency_key":"error-gates","mode":{"kind":"private","pool_id":"circle"},"workload":{"kind":"sha256","texts":["x"]}})).unwrap(),
        soapbox::model::now(),
    ).unwrap().id;
    let router = api::router(state.clone());
    for (path, token, expected) in [
        ("/v1/jobs/missing".to_string(), BUYER, 404),
        ("/v1/artifacts/missing".to_string(), BUYER, 404),
        (format!("/v1/jobs/{id}"), WORKER, 403),
        ("/v1/settlements".to_string(), WORKER, 403),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
    }
    state.store.lock().unwrap().db.execute("UPDATE jobs SET data=?1 WHERE id=?2",
        rusqlite::params![r#"{"status":"private-provider-key","spec":{"mode":{"kind":"https://provider.example/private-provider-key"}}}"#,id]).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri(format!("/v1/jobs/{id}"))
                .header("authorization", format!("Bearer {BUYER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 400);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("request could not be processed; reference="));
    assert!(!body.contains("private-provider-key"));
    assert!(!body.contains("provider.example"));
    assert!(!body.contains("missing field"));
}

#[tokio::test]
async fn failing_keyed_rpc_is_redacted_in_observe_and_refresh() {
    use http_body_util::BodyExt;
    use soapbox::payment::PaymentConfig;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc_url = format!(
        "http://{}/v2/private-provider-key?secret=private-query-key",
        listener.local_addr().unwrap()
    );
    let rpc = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().fallback(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "upstream-private-response",
                )
            }),
        )
        .await
        .unwrap()
    });
    let config: PaymentConfig = serde_json::from_value(json!({
        "rpc_url":rpc_url,"chain_id":31337,"token":format!("0x{}","1".repeat(40)),
        "escrow":format!("0x{}","2".repeat(40)),"escrow_code_sha256":"a".repeat(64),
        "microusdc_per_item":1,"confirmations":1,
        "wallets":{"buyer":format!("0x{}","3".repeat(40)),"worker":format!("0x{}","4".repeat(40))}
    }))
    .unwrap();
    let state = app(std::path::Path::new(":memory:"));
    let job = {
        let mut store = state.store.lock().unwrap();
        store.payments = Some(config.clone());
        store.submit(&principal("buyer", &["submitter"]),
            serde_json::from_value(json!({"idempotency_key":"rpc-disclosure","mode":{"kind":"paid_usdc","operator":"worker","max_cost_microusdc":10},"workload":{"kind":"sha256","texts":["x"]}})).unwrap(),soapbox::model::now()).unwrap()
    };
    let failure = config
        .observe(job.payment.as_ref().unwrap())
        .await
        .err()
        .unwrap();
    let details = format!("{failure:#}");
    assert!(!details.contains("private-provider-key"));
    assert!(!details.contains("private-query-key"));
    let response = api::router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/jobs/{}/payment", job.id))
                .header("authorization", format!("Bearer {BUYER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 503);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("payment verification unavailable; reference="));
    for secret in [
        "private-provider-key",
        "private-query-key",
        "upstream-private-response",
        &rpc_url,
    ] {
        assert!(!body.contains(secret));
    }
    rpc.abort();
}
