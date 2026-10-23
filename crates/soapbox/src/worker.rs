use crate::model::*;
use crate::sandbox_health::{Failure, Health};
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
    let failure = if config.sandbox.is_some() {
        crate::sandbox::check(config).await.err()
    } else {
        None
    };
    let (offer, failure) = inventory_cached(config, None, failure.as_ref()).await?;
    if let Some(failure) = failure {
        eprintln!("Sandbox diagnostic (local only): {}", failure.diagnostic);
    }
    Ok(offer)
}
async fn inventory_cached(
    config: &WorkerConfig,
    cached: Option<&crate::gpu::Runtime>,
    isolation_failure: Option<&Failure>,
) -> Result<(Offer, Option<Failure>)> {
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
    if let Some(error) = isolation_failure {
        pause_for_isolation(&mut offer, error);
        return Ok((offer, Some(error.clone())));
    }
    let mut boundary_failure = None;
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
            Ok(Err(error)) => {
                if let Some(failure) = error
                    .downcast_ref::<Failure>()
                    .filter(|e| e.blocks_execution())
                {
                    pause_for_isolation(&mut offer, failure);
                    boundary_failure = Some(failure.clone());
                } else {
                    if let Some(failure) = error.downcast_ref::<Failure>() {
                        eprintln!("Sandbox diagnostic (local only): {}", failure.diagnostic);
                    }
                    offer.gpu_unavailable_reason = Some(error.to_string());
                }
            }
            Err(_) => offer.gpu_unavailable_reason = Some("model warmup timed out".into()),
        }
    } else if paused {
        offer.gpu_unavailable_reason = Some("worker paused or RAM allowance unavailable".into());
    }
    Ok((offer, boundary_failure))
}
fn pause_for_isolation(offer: &mut Offer, error: &Failure) {
    offer.paused = true;
    offer.isolation = None;
    offer.gpu_observed = false;
    offer.runtime = None;
    offer.models.clear();
    // Keep the existing wire schema for old coordinators/desktop clients. The
    // stable prefix describes an execution failure, despite the legacy name.
    offer.gpu_unavailable_reason = Some(error.to_string());
}
async fn execute(
    job: &Job,
    config: &WorkerConfig,
    reservation: Option<&crate::sandbox::GpuReservation>,
) -> Result<Value> {
    if config.sandbox.is_some() {
        return crate::sandbox::execute_reserved(Some(job), config, reservation).await;
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
    let mut health = Health::default();
    let mut cached_probe: Option<(std::time::Instant, crate::gpu::Runtime)> = None;
    let mut previous_offer: Option<Offer> = None;
    loop {
        // Fresh RAM/GPU admission on every heartbeat; expensive isolated model probes at most once per minute.
        let cached = cached_probe
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
            .map(|(_, runtime)| runtime.clone());
        let mut offer = {
            let probe = async {
                let failure = if config.sandbox.is_some() {
                    health
                        .check(std::time::Instant::now(), || crate::sandbox::check(&config))
                        .await
                } else {
                    None
                };
                let (offer, boundary_failure) =
                    inventory_cached(&config, cached.as_ref(), failure.as_ref()).await?;
                if failure.is_none() {
                    if let Some(error) = boundary_failure {
                        health.invalidate(std::time::Instant::now(), error);
                    }
                }
                Ok::<_, anyhow::Error>(offer)
            };
            tokio::pin!(probe);
            let mut heartbeat = tokio::time::interval(Duration::from_secs(8));
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    result = &mut probe => break result?,
                    _ = heartbeat.tick() => {
                        if let Some(mut pending) = previous_offer.clone() {
                            // Remain visible while probing, but do not advertise admission readiness.
                            pending.paused = true;
                            pending.gpu_observed = false;
                            pending.gpu_unavailable_reason = Some("Checking sandbox and runtime readiness".into());
                            client.post::<Value>("/v1/workers/register", &pending).await?;
                        }
                    },
                    _ = tokio::signal::ctrl_c() => return Ok(()),
                }
            }
        };
        if offer.isolation.is_none() && config.sandbox.is_some() {
            cached_probe = None;
        }
        previous_offer = Some(offer.clone());
        if config.sandbox.is_some() && cached.is_none() {
            cached_probe = offer
                .runtime
                .clone()
                .map(|runtime| (std::time::Instant::now(), runtime));
        }
        // A cached probe is not an exclusive reservation. Hold the same host lock
        // from admission through execution so another coordinator's worker cannot
        // consume the device between registration and claim.
        let gpu_reservation = if offer.gpu_observed {
            if let Some(sandbox) = &config.sandbox {
                match crate::sandbox::reserve_gpu(sandbox) {
                    Ok(held) => Some(held),
                    Err(error) => {
                        offer.gpu_observed = false;
                        offer.models.clear();
                        offer.runtime = None;
                        offer.gpu_unavailable_reason = Some(error.to_string());
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        client.post::<Value>("/v1/workers/register", &offer).await?;
        if offer.paused {
            if once {
                return Ok(());
            }
            tokio::select! {
                _=tokio::signal::ctrl_c()=>return Ok(()),
                _=tokio::time::sleep(Duration::from_secs(2))=>{},
            }
            continue;
        }
        if let Some(job) = client
            .post::<Option<Job>>("/v1/workers/claim", json!({}))
            .await?
        {
            let lease = job.lease_id.clone().context("lease missing")?;
            let execution = execute(&job, &config, gpu_reservation.as_ref());
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
                    if let Some(failure) = error.downcast_ref::<Failure>() {
                        eprintln!("Sandbox diagnostic (local only): {}", failure.diagnostic);
                        if failure.blocks_execution() {
                            pause_for_isolation(&mut offer, failure);
                            cached_probe = None;
                            health.invalidate(std::time::Instant::now(), failure.clone());
                            client.post::<Value>("/v1/workers/register", &offer).await?;
                        }
                    }
                    client
                        .post::<Value>(
                            &format!("/v1/jobs/{}/fail", job.id),
                            json!({"lease_id":lease}),
                        )
                        .await?;
                }
            }
        }
        drop(gpu_reservation);
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
    async fn isolation_failure_disables_cpu_and_gpu_and_recovery_preserves_manual_pause() {
        let dir = tempfile::tempdir().unwrap();
        let pause = dir.path().join("PAUSE");
        let config: WorkerConfig = serde_json::from_value(json!({
            "sandbox":{"image":format!("sha256:{}", "a".repeat(64)),"models":dir.path(),"gpu_lock":dir.path().join("gpu.lock")},
            "modes":["private"],"pools":["team"],"memory_mib":256,"pause_file":pause
        })).unwrap();
        let error = Failure::new(
            crate::sandbox_health::Code::SwapUnenforced,
            "private diagnostic",
        );
        let (offer, _) = inventory_cached(&config, None, Some(&error)).await.unwrap();
        let spec: Submit = serde_json::from_value(json!({
            "idempotency_key":"health-test", "mode":{"kind":"private","pool_id":"team"},
            "workload":{"kind":"sha256","texts":["sample"]},
            "resources":{"cpu_threads":1,"memory_mib":64,"require_gpu":false}
        }))
        .unwrap();
        assert!(offer.paused);
        assert!(offer.isolation.is_none());
        assert!(!offer.accepts(&spec));
        let mut gpu = spec.clone();
        gpu.resources.require_gpu = true;
        gpu.workload = Workload::Embeddings {
            model: "test:latest".into(),
            model_digest: Some("a".repeat(64)),
            texts: vec!["sample".into()],
        };
        gpu.validate().unwrap();
        // Both jobs are admissible on a healthy offer before isolation fails.
        let mut healthy = offer.clone();
        healthy.paused = false;
        healthy.isolation = Some("docker_v1".into());
        healthy.gpu_observed = true;
        healthy.models = vec!["test:latest".into()];
        healthy.runtime = Some(crate::gpu::Runtime {
            model: "test:latest".into(),
            model_digest: "a".repeat(64),
            version: "test".into(),
            model_bytes: 100,
            gpu_bytes: 100,
            observed_at: now(),
        });
        assert!(healthy.accepts(&spec));
        assert!(healthy.accepts(&gpu));
        pause_for_isolation(&mut healthy, &error);
        assert!(!healthy.accepts(&spec));
        assert!(!healthy.accepts(&gpu));
        assert!(healthy.runtime.is_none() && healthy.models.is_empty());
        assert!(!offer.accepts(&gpu));
        assert!(!offer
            .gpu_unavailable_reason
            .unwrap()
            .contains("private diagnostic"));
        std::fs::write(&pause, "operator paused").unwrap();
        let (recovered, _) = inventory_cached(&config, None, None).await.unwrap();
        assert!(recovered.paused);
        assert!(pause.exists());
        assert!(!recovered.accepts(&spec));
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
