use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use soapbox::{
    api::{self, App},
    config::Principal,
    model::digest,
    store::Store,
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

async fn call(router: &axum::Router, token: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(if body.is_some() { "POST" } else { "GET" })
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn runtime_peer_admission_enforces_roles_scope_owner_paid_block_and_revocation() {
    let admin = "admin-test-0000000000000000000000000";
    let key = "ab".repeat(32);
    let token = "cd".repeat(32);
    let store = Store::open(std::path::Path::new(":memory:"), 100, 4096).unwrap();
    let state = App {
        store: Arc::new(Mutex::new(store)),
        identities: Arc::new(vec![(
            digest(admin.as_bytes()),
            Principal {
                id: "admin".into(),
                token_env: String::new(),
                roles: vec!["admin".into(), "submitter".into()],
                pools: vec!["team".into()],
                projects: vec!["science".into()],
            },
        )]),
    };
    let router = api::router(state.clone());
    let grant = json!({"public_key":key,"token":token,"pools":["team"],"projects":["science"]});
    assert_eq!(
        call(
            &router,
            admin,
            "/v1/admissions",
            Some(json!({"public_key":key,"token":token,"pools":["outside"],"projects":[]}))
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(&router, admin, "/v1/admissions", Some(grant.clone()))
            .await
            .0,
        200
    );
    assert_eq!(
        call(&router, &token, "/v1/admissions", Some(grant)).await.0,
        403
    );
    let spec = json!({"idempotency_key":"test","mode":{"kind":"private","pool_id":"team"},"workload":{"kind":"sha256","texts":["private input"]}});
    let (status, job) = call(&router, &token, "/v1/jobs", Some(spec.clone())).await;
    assert_eq!(status, 200);
    assert_eq!(
        call(&router, &token, "/v1/jobs", Some(spec)).await.1["id"],
        job["id"]
    );
    for mode in [
        json!({"kind":"paid","max_cost_credits":1}),
        json!({"kind":"private","pool_id":"outside"}),
    ] {
        assert_ne!(call(&router,&token,"/v1/jobs",Some(json!({"idempotency_key":"forbidden","mode":mode,"workload":{"kind":"sha256","texts":["x"]}}))).await.0,200);
    }
    let second = "ef".repeat(32);
    assert_eq!(
        call(
            &router,
            admin,
            "/v1/admissions",
            Some(
                json!({"public_key":"aa".repeat(32),"token":second,"pools":["team"],"projects":[]})
            )
        )
        .await
        .0,
        200
    );
    assert_eq!(
        call(
            &router,
            &second,
            &format!("/v1/jobs/{}", job["id"].as_str().unwrap()),
            None
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(
            &router,
            admin,
            &format!("/v1/admissions/{key}/revoke"),
            Some(json!({}))
        )
        .await
        .0,
        200
    );
    assert_eq!(call(&router, &token, "/v1/status", None).await.0, 401);
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .job(job["id"].as_str().unwrap())
            .unwrap()
            .status,
        "cancelled"
    );
}
