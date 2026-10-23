//! Real worker + HTTP coordinator; deterministic child-only Docker fault injection.
//! This never changes the host Docker daemon, PATH, cgroups or wallet state.
#![cfg(unix)]
use serde_json::{json, Value};
use soapbox::{
    api::{self, App},
    config::Principal,
    model::{digest, now},
    store::Store,
    worker::ApiClient,
};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
};

async fn status(client: &ApiClient) -> Value {
    client
        .request(reqwest::Method::GET, "/v1/status", None)
        .await
        .unwrap()
}
async fn wait_status(client: &ApiClient, predicate: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let value = status(client).await;
            if predicate(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("worker state did not converge")
}
fn paused(value: &Value) -> bool {
    value["nodes"][0]["offer"]["paused"] == true
}
#[tokio::test]
async fn failed_isolation_recovers_without_claiming_work_or_overriding_operator_pauses() {
    let dir = tempfile::tempdir().unwrap();
    let mode = dir.path().join("mode");
    let pause = dir.path().join("PAUSE");
    let log = dir.path().join("docker-calls");
    std::fs::write(&mode, "broken").unwrap();
    let shim = dir.path().join("docker");
    std::fs::write(
        &shim,
        format!(
            r#"#!/bin/sh
[ "$1" = "rm" ] && exit 0
for last do :; done
cat >/dev/null
printf '%s\n' "$last" >> "$TEST_DOCKER_CALLS"
if [ "$(cat "$TEST_DOCKER_MODE")" = "broken" ]; then
  i=0
  while [ "$i" -lt 500 ]; do echo 'WARNING: noisy Docker diagnostic' >&2; i=$((i+1)); done
  echo 'SOAPBOX_SANDBOX_ERROR "sandbox_swap_unenforced"' >&2
  exit 1
fi
if [ "$last" = "sandbox-check" ]; then
  echo '{{"isolation":"docker_v1"}}'
else
  echo '{{"hashes":["{}"]}}'
fi
"#,
            digest(b"sample")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = dir.path().join("worker.json");
    std::fs::write(&config, json!({
        "sandbox":{"image":format!("sha256:{}","a".repeat(64)),"models":dir.path(),"gpu_lock":dir.path().join("gpu.lock")},
        "modes":["private"],"pools":["team"],"memory_mib":256,"pause_file":pause
    }).to_string()).unwrap();
    let token = "w".repeat(64);
    let principal = Principal {
        id: "test-worker".into(),
        token_env: String::new(),
        roles: vec!["worker".into(), "admin".into(), "submitter".into()],
        pools: vec!["team".into()],
        projects: vec![],
    };
    let store = Arc::new(Mutex::new(
        Store::open(&dir.path().join("state.db"), 100, 1048576).unwrap(),
    ));
    let app = App {
        store: store.clone(),
        identities: Arc::new(vec![(digest(token.as_bytes()), principal.clone())]),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, api::router(app)).await.unwrap();
    });
    let client = ApiClient::new(&url, token.clone()).unwrap();
    let job: Value = client
        .post(
            "/v1/jobs",
            json!({
                "idempotency_key":"isolation-recovery", "mode":{"kind":"private","pool_id":"team"},
                "workload":{"kind":"sha256","texts":["sample"]},
                "resources":{"cpu_threads":1,"memory_mib":64,"require_gpu":false}
            }),
        )
        .await
        .unwrap();
    let output = std::fs::File::create(dir.path().join("worker.log")).unwrap();
    let mut worker = tokio::process::Command::new(env!("CARGO_BIN_EXE_soapbox"))
        .args([
            "worker",
            "--config",
            config.to_str().unwrap(),
            "--coordinator",
            &url,
        ])
        .env("SOAPBOX_WORKER_TOKEN", &token)
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("TEST_DOCKER_MODE", &mode)
        .env("TEST_DOCKER_CALLS", &log)
        .stdout(std::process::Stdio::null())
        .stderr(output)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let failed = wait_status(&client, |s| {
        paused(s)
            && s["nodes"][0]["offer"]["gpu_unavailable_reason"]
                .as_str()
                .is_some_and(|s| s.starts_with("sandbox_swap_unenforced:"))
    })
    .await;
    assert_eq!(failed["jobs"][0]["status"], "queued");
    assert_eq!(failed["jobs"][0]["attempts"], 0);
    assert!(!std::fs::read_to_string(&log)
        .unwrap()
        .contains("sandbox-run"));
    assert!(failed["nodes"][0]["offer"]["isolation"].is_null());
    std::fs::write(&pause, "manual pause").unwrap();
    store.lock().unwrap().pause("test-worker", true).unwrap();
    std::fs::write(&mode, "healthy").unwrap();
    let recovered = wait_status(&client, |s| {
        paused(s) && s["nodes"][0]["offer"]["isolation"] == "docker_v1"
    })
    .await;
    assert_eq!(recovered["jobs"][0]["attempts"], 0);
    assert!(pause.exists());
    std::fs::remove_file(&pause).unwrap();
    let still_admin_paused = wait_status(&client, |s| {
        !paused(s) && s["nodes"][0]["operator_paused"] == true
    })
    .await;
    assert_eq!(still_admin_paused["jobs"][0]["attempts"], 0);
    store.lock().unwrap().pause("test-worker", false).unwrap();
    let done = wait_status(&client, |s| s["jobs"][0]["status"] == "succeeded").await;
    assert_eq!(done["jobs"][0]["id"], job["id"]);
    assert_eq!(done["jobs"][0]["attempts"], 1);
    // Environment regresses after successful setup/execution: capacity must be withdrawn again.
    std::fs::write(&mode, "broken").unwrap();
    wait_status(&client, paused).await;
    let fresh = store.lock().unwrap().nodes(&principal, now()).unwrap();
    assert_eq!(fresh[0]["offer"]["paused"], true);
    assert!(worker.try_wait().unwrap().is_none());
    worker.kill().await.unwrap();
    worker.wait().await.unwrap();
    server.abort();
    let diagnostic = std::fs::read_to_string(dir.path().join("worker.log")).unwrap();
    assert!(diagnostic.contains("Compute sandbox isolation recovered"));
    assert!(diagnostic.contains("WARNING: noisy Docker diagnostic"));
}
