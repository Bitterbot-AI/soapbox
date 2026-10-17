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
    #[serde(default)]
    pub sandbox: Option<crate::sandbox::SandboxConfig>,
    #[serde(default)]
    pub allow_unsafe_host: bool,
    pub modes: Vec<String>,
    #[serde(default)]
    pub pools: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    pub memory_mib: u64,
    #[serde(default)]
    pub ollama_model: Option<String>,
    #[serde(default)]
    pub ollama_digest: Option<String>,
    #[serde(default = "default_temperature")]
    pub max_gpu_temperature_c: u32,
    #[serde(default = "default_utilization")]
    pub max_gpu_utilization_pct: u32,
    #[serde(default = "default_free")]
    pub min_gpu_free_mib: u64,
    #[serde(default)]
    pub pause_file: Option<PathBuf>,
    #[serde(default)]
    pub min_price_credits: u64,
}
fn default_temperature() -> u32 {
    80
}
fn default_utilization() -> u32 {
    40
}
fn default_free() -> u64 {
    256
}
impl WorkerConfig {
    pub fn check_gpu_policy(&self, hardware: &[crate::gpu::Gpu]) -> Result<()> {
        ensure!(!hardware.is_empty(), "NVIDIA telemetry unavailable");
        ensure!(
            hardware
                .iter()
                .all(|g| g.temperature_c < self.max_gpu_temperature_c),
            "GPU temperature exceeds admission limit"
        );
        ensure!(
            hardware
                .iter()
                .all(|g| g.utilization_pct <= self.max_gpu_utilization_pct),
            "GPU busy with existing work"
        );
        ensure!(
            hardware
                .iter()
                .all(|g| g.memory_free_mib >= self.min_gpu_free_mib),
            "insufficient free GPU memory"
        );
        Ok(())
    }
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if let Some(sandbox) = &config.sandbox {
            sandbox.validate()?;
        }
        ensure!(
            config.sandbox.is_some() || config.allow_unsafe_host,
            "sandbox required; allow_unsafe_host is for development only"
        );
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
        ensure!(
            (40..=90).contains(&config.max_gpu_temperature_c)
                && config.max_gpu_utilization_pct <= 100
                && config.min_gpu_free_mib >= 128,
            "invalid GPU admission policy"
        );
        if config.ollama_model.is_some() {
            ensure!(
                config
                    .ollama_digest
                    .as_ref()
                    .is_some_and(|d| crate::gpu::valid_digest(d)),
                "ollama_model requires a pinned ollama_digest"
            );
        }
        Ok(config)
    }
}
pub async fn inventory(config: &WorkerConfig) -> Result<Offer> {
    inventory_cached(config, None).await
}
async fn inventory_cached(
    config: &WorkerConfig,
    cached: Option<&crate::gpu::Runtime>,
) -> Result<Offer> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let available = system.available_memory() / 1024 / 1024;
    let paused =
        available < config.memory_mib || config.pause_file.as_ref().is_some_and(|p| p.exists());
    let mut offer = Offer {
        isolation: config.sandbox.as_ref().map(|_| "docker_v1".to_string()),
        cpu_threads: 1,
        memory_mib: config.memory_mib.min(available).max(64),
        gpu_observed: false,
        models: vec![],
        modes: config.modes.clone(),
        pools: config.pools.clone(),
        projects: config.projects.clone(),
        paused,
        min_price_credits: config.min_price_credits,
        runtime: None,
        hardware: vec![],
        gpu_unavailable_reason: None,
    };
    if config.ollama_model.is_some() && !paused {
        offer.hardware = crate::gpu::inventory().await.unwrap_or_default();
        let readiness = async {
            config.check_gpu_policy(&offer.hardware)?;
            let runtime: crate::gpu::Runtime = if let Some(runtime) = cached {
                runtime.clone()
            } else if config.sandbox.is_some() {
                serde_json::from_value(crate::sandbox::execute(None, config).await?)?
            } else {
                crate::ollama::ready(crate::ollama::LOCAL_BACKEND, config).await?
            };
            ensure!(runtime.fully_gpu(), "model is not fully GPU resident");
            Ok::<_, anyhow::Error>(runtime)
        };
        match tokio::time::timeout(Duration::from_secs(100), readiness).await {
            Ok(Ok(runtime)) => {
                offer.gpu_observed = true;
                offer.models.push(runtime.model.clone());
                offer.runtime = Some(runtime);
            }
            Ok(Err(error)) => offer.gpu_unavailable_reason = Some(error.to_string()),
            Err(_) => offer.gpu_unavailable_reason = Some("model warmup timed out".into()),
        }
    } else if paused {
        offer.gpu_unavailable_reason = Some("worker paused or RAM allowance unavailable".into());
    }
    Ok(offer)
}
async fn execute(job: &Job, config: &WorkerConfig) -> Result<Value> {
    if config.sandbox.is_some() {
        return crate::sandbox::execute(Some(job), config).await;
    }
    ensure!(
        config.allow_unsafe_host && job.payment.is_none(),
        "USDC jobs require isolation"
    );
    if let Some(result) = job.spec.workload.cpu_result() {
        return Ok(result);
    }
    tokio::time::timeout(
        Duration::from_secs(100),
        crate::ollama::execute(job, config, crate::ollama::LOCAL_BACKEND),
    )
    .await?
}
pub async fn run(client: ApiClient, config: WorkerConfig, once: bool) -> Result<()> {
    let mut cached_probe: Option<(std::time::Instant, crate::gpu::Runtime)> = None;
    loop {
        // Fresh RAM/GPU admission on every heartbeat; expensive isolated model probes at most once per minute.
        let cached = cached_probe
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
            .map(|(_, runtime)| runtime);
        let offer = inventory_cached(&config, cached).await?;
        if config.sandbox.is_some() && cached.is_none() {
            cached_probe = offer
                .runtime
                .clone()
                .map(|runtime| (std::time::Instant::now(), runtime));
        }
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
                        if let Err(error) = client.post::<Value>(&format!("/v1/jobs/{}/renew",job.id),json!({"lease_id":lease})).await {
                            // Drain the bounded inference request before another claim; never settle a revoked lease.
                            // Dropping the sandbox future forcibly removes its container.
                            if config.sandbox.is_none() { let _ = execution.await; }
                            return Err(error);
                        }
                        // The registration remains fresh during a lease without probing/warming another inference.
                        client.post::<Value>("/v1/workers/register", &offer).await?;
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
