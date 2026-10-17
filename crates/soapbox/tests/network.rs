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
