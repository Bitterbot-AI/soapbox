use crate::model::*;
use anyhow::{bail, ensure, Context, Result};
use reqwest::{Client, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base: Url,
    token: String,
}
impl ApiClient {
    pub fn new(url: &str, token: String) -> Result<Self> {
        ensure!(token.len() >= 32, "token must be at least 32 characters");
        let base = Url::parse(url)?;
        ensure!(
            base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none(),
            "invalid coordinator URL"
        );
        ensure!(
            base.path() == "/",
            "coordinator URL must not contain a path"
        );
        let local = matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        ensure!(
            base.scheme() == "https" || (base.scheme() == "http" && local),
            "remote coordinators require HTTPS"
        );
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            base,
            token,
        })
    }
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        ensure!(
            path.starts_with("/v1/")
                && path
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/_-".contains(&c)),
            "invalid API path"
        );
        let mut request = self
            .client
            .request(method, self.base.join(path)?)
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 32 * 1024 * 1024,
                "coordinator response too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| status.to_string());
            bail!("coordinator rejected request: {message}");
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: impl Serialize) -> Result<T> {
        self.request(
            reqwest::Method::POST,
            path,
            Some(serde_json::to_value(body)?),
        )
        .await
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    pub modes: Vec<String>,
    #[serde(default)]
    pub pools: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    pub memory_mib: u64,
    #[serde(default)]
    pub ollama_model: Option<String>,
    #[serde(default)]
    pub pause_file: Option<PathBuf>,
    #[serde(default)]
    pub min_price_credits: u64,
}
impl WorkerConfig {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            config.memory_mib >= 256,
            "worker memory allowance must be at least 256 MiB"
        );
        ensure!(
            !config.modes.is_empty()
                && config
                    .modes
                    .iter()
                    .all(|m| ["paid", "private", "collective"].contains(&m.as_str())),
            "explicit allowed modes required"
        );
        Ok(config)
    }
}
pub async fn inventory(config: &WorkerConfig) -> Result<Offer> {
    inventory_from(config, "http://127.0.0.1:11434").await
}
async fn inventory_from(config: &WorkerConfig, backend: &str) -> Result<Offer> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let available = system.available_memory() / 1024 / 1024;
    let mut models = Vec::new();
    let mut gpu = false;
    if let Some(model) = &config.ollama_model {
        let response = Client::builder()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .get(format!("{backend}/api/ps"))
            .send()
            .await;
        if let Ok(response) = response {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    if let Some(entries) = value.get("models").and_then(Value::as_array) {
                        if let Some(entry) = entries.iter().find(|v| {
                            v.get("name").and_then(Value::as_str) == Some(model.as_str())
                                || v.get("model").and_then(Value::as_str) == Some(model.as_str())
                        }) {
                            models.push(model.clone());
                            gpu = entry
                                .get("size_vram")
                                .and_then(Value::as_u64)
                                .is_some_and(|n| n > 0);
                        }
                    }
                }
            }
        }
    }
    Ok(Offer {
        cpu_threads: 1,
        memory_mib: config.memory_mib.min(available).max(64),
        gpu_observed: gpu,
        models,
        modes: config.modes.clone(),
        pools: config.pools.clone(),
        projects: config.projects.clone(),
        paused: available < config.memory_mib
            || config.pause_file.as_ref().is_some_and(|p| p.exists()),
        min_price_credits: config.min_price_credits,
    })
}
async fn execute(job: &Job, config: &WorkerConfig) -> Result<Value> {
    execute_with_backend(job, config, "http://127.0.0.1:11434").await
}
async fn execute_with_backend(job: &Job, config: &WorkerConfig, backend: &str) -> Result<Value> {
    if let Some(result) = job.spec.workload.cpu_result() {
        return Ok(result);
    }
    let Workload::Embeddings { model, texts } = &job.spec.workload else {
        bail!("unsupported workload")
    };
    ensure!(
        config.ollama_model.as_ref() == Some(model),
        "model not allowed"
    );
    if job.spec.resources.require_gpu {
        ensure!(
            inventory_from(config, backend).await?.gpu_observed,
            "configured model is not GPU resident"
        );
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(100))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client
        .post(format!("{backend}/api/embed"))
        .json(&json!({"model":model,"input":texts,"truncate":false,"keep_alive":"5m"}))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2_097_152,
            "embedding response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let result = json!({"embeddings":value.get("embeddings").context("missing embeddings")?});
    job.spec.workload.verify(&result)?;
    Ok(result)
}
pub async fn run(client: ApiClient, config: WorkerConfig, once: bool) -> Result<()> {
    loop {
        let offer = inventory(&config).await?;
        client.post::<Value>("/v1/workers/register", &offer).await?;
        if let Some(job) = client
            .post::<Option<Job>>("/v1/workers/claim", json!({}))
            .await?
        {
            let lease = job.lease_id.clone().context("lease missing")?;
            let execution = execute(&job, &config);
            tokio::pin!(execution);
            let mut ticker = tokio::time::interval(Duration::from_secs(8));
            ticker.tick().await;
            let result = loop {
                tokio::select! {
                    result=&mut execution=>break result,
                    _=ticker.tick()=>{
                        // Failure to renew stops local execution. The coordinator owns retry decisions.
                        client.post::<Value>(&format!("/v1/jobs/{}/renew",job.id),json!({"lease_id":lease})).await?;
                        client.post::<Value>("/v1/workers/register",inventory(&config).await?).await?;
                    },
                    _=tokio::signal::ctrl_c()=>return Ok(()),
                }
            };
            match result {
                Ok(result) => {
                    // Completion is idempotent, so transient response loss cannot charge twice.
                    let completion = Completion {
                        lease_id: lease,
                        result,
                    };
                    let path = format!("/v1/jobs/{}/complete", job.id);
                    let mut done = false;
                    for _ in 0..3 {
                        if client.post::<Value>(&path, &completion).await.is_ok() {
                            done = true;
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    ensure!(
                        done,
                        "completion could not be acknowledged; lease will expire"
                    );
                }
                Err(error) => {
                    eprintln!("job {} failed: {error}", job.id);
                    client
                        .post::<Value>(
                            &format!("/v1/jobs/{}/fail", job.id),
                            json!({"lease_id":lease}),
                        )
                        .await?;
                }
            }
        }
        if once {
            return Ok(());
        }
        tokio::select! {
            _=tokio::signal::ctrl_c()=>return Ok(()),
            _=tokio::time::sleep(Duration::from_secs(2))=>{},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    fn config() -> WorkerConfig {
        WorkerConfig {
            modes: vec!["private".into()],
            pools: vec!["circle".into()],
            projects: vec![],
            memory_mib: 256,
            ollama_model: Some("test-model:latest".into()),
            pause_file: None,
            min_price_credits: 0,
        }
    }
    fn job() -> Job {
        Job {
            id: "test".into(),
            owner: "buyer".into(),
            spec: Submit {
                idempotency_key: "test".into(),
                mode: Mode::Private {
                    pool_id: "circle".into(),
                },
                workload: Workload::Embeddings {
                    model: "test-model:latest".into(),
                    texts: vec!["one".into()],
                },
                resources: Resources {
                    cpu_threads: 1,
                    memory_mib: 256,
                    require_gpu: true,
                },
            },
            status: "running".into(),
            attempts: 1,
            created_at: 0,
            deadline: 600,
            worker: Some("w".into()),
            lease_id: Some("lease".into()),
            lease_until: Some(30),
            run_started_at: Some(0),
            result: None,
            error: None,
            receipt: None,
            reserved_credits: 0,
        }
    }
    #[tokio::test]
    async fn configured_embedding_backend_is_executed_and_gpu_offer_is_observed() {
        let app = Router::new()
            .route(
                "/api/ps",
                get(|| async {
                    Json(json!({"models":[{"name":"test-model:latest","size_vram":100}]}))
                }),
            )
            .route(
                "/api/embed",
                post(|Json(body): Json<Value>| async move {
                    assert_eq!(body["model"], "test-model:latest");
                    assert_eq!(body["input"], json!(["one"]));
                    assert_eq!(body["truncate"], false);
                    Json(json!({"embeddings":[[0.25,0.75]]}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let offer = inventory_from(&config(), &base).await.unwrap();
        assert!(offer.gpu_observed);
        assert_eq!(offer.models, ["test-model:latest"]);
        let result = execute_with_backend(&job(), &config(), &base)
            .await
            .unwrap();
        assert_eq!(result["embeddings"], json!([[0.25, 0.75]]));
        task.abort();
    }
    #[tokio::test]
    async fn missing_model_never_satisfies_gpu_job() {
        let app = Router::new().route("/api/ps", get(|| async { Json(json!({"models":[]})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(!inventory_from(&config(), &base).await.unwrap().gpu_observed);
        assert!(execute_with_backend(&job(), &config(), &base)
            .await
            .is_err());
        task.abort();
    }
    #[tokio::test]
    async fn api_paths_cannot_redirect_credentials() {
        let client = ApiClient::new("http://127.0.0.1:1", "x".repeat(32)).unwrap();
        assert!(client
            .request::<Value>(reqwest::Method::GET, "//other.example/v1/status", None)
            .await
            .unwrap_err()
            .to_string()
            .contains("path"));
    }
}
