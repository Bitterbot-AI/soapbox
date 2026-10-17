//! Fixed loopback inference backend. Buyers cannot select URLs, commands or download models.
use crate::{
    gpu::{Execution, Runtime},
    model::*,
    worker::WorkerConfig,
};
use anyhow::{ensure, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const LOCAL_BACKEND: &str = "http://127.0.0.1:11434";
fn client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
async fn response(request: reqwest::RequestBuilder) -> Result<Value> {
    let mut response = request.send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2_097_152,
            "inference response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn get(backend: &str, path: &str) -> Result<Value> {
    response(
        client()?
            .get(format!("{backend}/api/{path}"))
            .timeout(Duration::from_secs(3)),
    )
    .await
}
fn entry<'a>(value: &'a Value, model: &str) -> Option<&'a Value> {
    value.get("models")?.as_array()?.iter().find(|v| {
        v.get("name").and_then(Value::as_str) == Some(model)
            || v.get("model").and_then(Value::as_str) == Some(model)
    })
}
async fn check_digest(backend: &str, model: &str, digest: &str) -> Result<()> {
    let tags = get(backend, "tags").await?;
    ensure!(
        entry(&tags, model)
            .and_then(|v| v.get("digest"))
            .and_then(Value::as_str)
            == Some(digest),
        "configured model is missing or its digest changed; operator must install the pinned model"
    );
    Ok(())
}
pub(crate) async fn resident(backend: &str, model: &str, digest: &str) -> Result<Option<Runtime>> {
    check_digest(backend, model, digest).await?;
    let version = get(backend, "version").await?;
    let version = version
        .get("version")
        .and_then(Value::as_str)
        .context("missing runtime version")?;
    ensure!(
        !version.is_empty() && version.len() <= 128,
        "invalid runtime version"
    );
    let ps = get(backend, "ps").await?;
    let Some(found) = entry(&ps, model) else {
        return Ok(None);
    };
    ensure!(
        found.get("digest").and_then(Value::as_str) == Some(digest),
        "resident model digest mismatch"
    );
    Ok(Some(Runtime {
        model: model.into(),
        model_digest: digest.into(),
        version: version.into(),
        model_bytes: found
            .get("size")
            .and_then(Value::as_u64)
            .context("missing model size")?,
        gpu_bytes: found
            .get("size_vram")
            .and_then(Value::as_u64)
            .context("missing GPU residency")?,
        observed_at: now(),
    }))
}
pub(crate) async fn ready(backend: &str, config: &WorkerConfig) -> Result<Runtime> {
    let model = config
        .ollama_model
        .as_deref()
        .context("model not configured")?;
    let digest = config
        .ollama_digest
        .as_deref()
        .context("pinned model digest required")?;
    ensure!(
        crate::gpu::valid_digest(digest),
        "invalid pinned model digest"
    );
    if let Some(runtime) = resident(backend, model, digest).await? {
        return Ok(runtime);
    }
    // Only a locally installed, allowlisted model may be warmed; no pull endpoint is exposed.
    response(client()?.post(format!("{backend}/api/embed")).json(
        &json!({"model":model,"input":["Soapbox GPU readiness probe"],"truncate":false,
            "keep_alive":"10m","options":{"num_gpu":999}}),
    ))
    .await?;
    resident(backend, model, digest)
        .await?
        .context("model did not become resident")
}
pub(crate) async fn execute(job: &Job, config: &WorkerConfig, backend: &str) -> Result<Value> {
    let hardware = crate::gpu::inventory().await.unwrap_or_default();
    execute_on(job, config, backend, hardware).await
}
async fn execute_on(
    job: &Job,
    config: &WorkerConfig,
    backend: &str,
    hardware: Vec<crate::gpu::Gpu>,
) -> Result<Value> {
    let Workload::Embeddings {
        model,
        texts,
        model_digest,
    } = &job.spec.workload
    else {
        anyhow::bail!("unsupported workload");
    };
    ensure!(
        config.ollama_model.as_ref() == Some(model),
        "model not allowed"
    );
    let pinned = config
        .ollama_digest
        .as_deref()
        .context("worker model digest not pinned")?;
    ensure!(
        model_digest.as_deref().is_none_or(|d| d == pinned),
        "requested model digest not allowed"
    );
    let start = Instant::now();
    let before = ready(backend, config).await?;
    if job.spec.resources.require_gpu {
        ensure!(
            before.fully_gpu(),
            "configured model is not fully GPU resident"
        );
        config.check_gpu_policy(&hardware)?;
    }
    let mut rows = Vec::new();
    let mut total = 0u64;
    let mut load = 0u64;
    let mut tokens = 0u64;
    let batches = texts.len().div_ceil(16);
    for batch in texts.chunks(16) {
        let value = response(client()?.post(format!("{backend}/api/embed")).json(
            &json!({"model":model,"input":batch,"truncate":false,"keep_alive":"10m","options":{"num_gpu":999}})
        )).await?;
        ensure!(
            value.get("model").and_then(Value::as_str) == Some(model.as_str()),
            "response model mismatch"
        );
        let embeddings = value
            .get("embeddings")
            .and_then(Value::as_array)
            .context("missing embeddings")?;
        ensure!(
            embeddings.len() == batch.len(),
            "backend batch count mismatch"
        );
        rows.extend(embeddings.iter().cloned());
        total = total
            .checked_add(
                value
                    .get("total_duration")
                    .and_then(Value::as_u64)
                    .context("missing duration")?,
            )
            .context("duration overflow")?;
        load = load
            .checked_add(
                value
                    .get("load_duration")
                    .and_then(Value::as_u64)
                    .context("missing load duration")?,
            )
            .context("duration overflow")?;
        tokens = tokens
            .checked_add(
                value
                    .get("prompt_eval_count")
                    .and_then(Value::as_u64)
                    .context("missing token count")?,
            )
            .context("token overflow")?;
    }
    let after = resident(backend, model, pinned)
        .await?
        .context("model unloaded during execution")?;
    ensure!(
        before.version == after.version,
        "runtime changed during execution"
    );
    let fully_gpu = before.fully_gpu() && after.fully_gpu() && !hardware.is_empty();
    ensure!(
        !job.spec.resources.require_gpu || fully_gpu,
        "GPU residency was lost during execution"
    );
    let dimensions = rows.first().and_then(Value::as_array).map_or(0, Vec::len);
    let execution = Execution {
        backend: "ollama".into(),
        model: model.clone(),
        model_digest: pinned.into(),
        runtime_version: after.version,
        fully_gpu_resident: fully_gpu,
        model_bytes: after.model_bytes,
        gpu_bytes: after.gpu_bytes,
        dimensions,
        batches,
        wall_ms: start.elapsed().as_millis() as u64,
        total_duration_ns: total,
        load_duration_ns: load,
        prompt_tokens: tokens,
        hardware,
    };
    execution.validate(model, pinned, dimensions)?;
    let result = json!({"embeddings":rows,"execution":execution});
    job.spec.verify_result(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn config() -> WorkerConfig {
        serde_json::from_value(
            json!({"allow_unsafe_host":true,"modes":["private"],"pools":["circle"],"memory_mib":256,
            "ollama_model":"test:latest","ollama_digest":"a".repeat(64)}),
        )
        .unwrap()
    }
    fn hardware() -> Vec<crate::gpu::Gpu> {
        vec![crate::gpu::Gpu {
            name: "test GPU".into(),
            uuid: "GPU-test".into(),
            driver: "test".into(),
            memory_total_mib: 16000,
            memory_free_mib: 14000,
            utilization_pct: 0,
            temperature_c: 45,
        }]
    }
    fn job() -> Job {
        Job {
            payment: None,
            id: "test".into(),
            owner: "buyer".into(),
            status: "running".into(),
            attempts: 1,
            created_at: 0,
            deadline: 600,
            worker: Some("worker".into()),
            lease_id: Some("lease".into()),
            lease_until: Some(30),
            run_started_at: Some(0),
            result: None,
            error: None,
            receipt: None,
            reserved_credits: 0,
            spec: Submit {
                idempotency_key: "test".into(),
                mode: Mode::Private {
                    pool_id: "circle".into(),
                },
                resources: Resources {
                    require_gpu: true,
                    ..Resources::default()
                },
                workload: Workload::Embeddings {
                    model: "test:latest".into(),
                    model_digest: Some("a".repeat(64)),
                    texts: (0..33).map(|n| n.to_string()).collect(),
                },
            },
        }
    }
    async fn backend(
        cpu: bool,
        drift: bool,
        unload: bool,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let ps = count.clone();
        let tags = count.clone();
        let app = Router::new()
            .route("/api/version",get(|| async {Json(json!({"version":"test-1"}))}))
            .route("/api/tags",get(move || {let count=tags.clone(); async move {
                let digest=if drift && count.load(Ordering::SeqCst)>0 {"b".repeat(64)} else {"a".repeat(64)};
                Json(json!({"models":[{"name":"test:latest","digest":digest}]}))
            }}))
            .route("/api/ps",get(move || {let count=ps.clone(); async move {
                let vram=if cpu || (unload && count.load(Ordering::SeqCst)>0) {0} else {100};
                Json(json!({"models":[{"name":"test:latest","digest":"a".repeat(64),"size":100,"size_vram":vram}]}))
            }}))
            .route("/api/embed",post(move |Json(body):Json<Value>| {let count=c.clone(); async move {
                count.fetch_add(1,Ordering::SeqCst);
                assert_eq!(body["truncate"],false);
                assert_eq!(body["options"]["num_gpu"],999);
                let rows:Vec<_>=body["input"].as_array().unwrap().iter().map(|s|
                    json!([s.as_str().unwrap().parse::<f64>().unwrap(),0.25])).collect();
                Json(json!({"model":"test:latest","embeddings":rows,"total_duration":2000000,
                    "load_duration":1000,"prompt_eval_count":16}))
            }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, task, count)
    }
    #[tokio::test]
    async fn gpu_batches_preserve_order_and_meter_bound_runtime() {
        let (url, task, count) = backend(false, false, false).await;
        let result = execute_on(&job(), &config(), &url, hardware())
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(result["embeddings"][32][0], 32.0);
        assert_eq!(result["execution"]["batches"], 3);
        assert_eq!(result["execution"]["total_duration_ns"], 6000000);
        assert_eq!(result["execution"]["fully_gpu_resident"], true);
        task.abort();
    }
    #[tokio::test]
    async fn cpu_fallback_digest_drift_and_lost_residency_never_complete_gpu_work() {
        for scenario in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let (url, task, _) = backend(scenario.0, scenario.1, scenario.2).await;
            assert!(execute_on(&job(), &config(), &url, hardware())
                .await
                .is_err());
            task.abort();
        }
    }
    #[tokio::test]
    async fn missing_telemetry_and_hot_or_busy_gpu_do_not_execute() {
        let (url, task, count) = backend(false, false, false).await;
        for condition in 0..4 {
            let mut gpus = hardware();
            match condition {
                0 => gpus.clear(),
                1 => gpus[0].temperature_c = 85,
                2 => gpus[0].utilization_pct = 80,
                _ => gpus[0].memory_free_mib = 100,
            }
            assert!(execute_on(&job(), &config(), &url, gpus).await.is_err());
        }
        assert_eq!(count.load(Ordering::SeqCst), 0);
        task.abort();
    }
}

#[cfg(test)]
mod warmup_tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    #[tokio::test]
    async fn installed_pinned_model_is_warmed_without_downloading() {
        let loaded = Arc::new(AtomicBool::new(false));
        let ps = loaded.clone();
        let warm = loaded.clone();
        let app=Router::new()
            .route("/api/tags",get(|| async {Json(json!({"models":[{"name":"test:latest","digest":"a".repeat(64)}]}))}))
            .route("/api/version",get(|| async {Json(json!({"version":"test"}))}))
            .route("/api/ps",get(move || {let loaded=ps.clone();async move {
                if loaded.load(Ordering::SeqCst) {
                    Json(json!({"models":[{"name":"test:latest","digest":"a".repeat(64),"size":100,"size_vram":100}]}))
                } else {Json(json!({"models":[]}))}
            }}))
            .route("/api/embed",post(move |Json(body):Json<Value>| {let loaded=warm.clone();async move {
                assert_eq!(body["input"],json!(["Soapbox GPU readiness probe"]));
                loaded.store(true,Ordering::SeqCst);
                Json(json!({"embeddings":[[0.1,0.2]]}))
            }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut config: WorkerConfig = serde_json::from_value(
            json!({"allow_unsafe_host":true,"modes":["private"],"memory_mib":256,
            "ollama_model":"test:latest","ollama_digest":"b".repeat(64)}),
        )
        .unwrap();
        assert!(ready(&base, &config).await.is_err());
        assert!(!loaded.load(Ordering::SeqCst));
        config.ollama_digest = Some("a".repeat(64));
        assert!(ready(&base, &config).await.unwrap().fully_gpu());
        assert!(loaded.load(Ordering::SeqCst));
        task.abort();
    }
}
