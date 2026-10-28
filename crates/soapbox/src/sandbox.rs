//! Operator-owned Docker executor. No buyer images, mounts, environment variables or commands.
use crate::sandbox_health::{diagnostic_tail, diagnostic_tail_until, Code, Failure};
use crate::{model::*, worker::WorkerConfig};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Immutable local Docker image ID, never a mutable tag.
    pub image: String,
    pub models: PathBuf,
    /// Host-wide lock for this dedicated GPU. Every worker on the host must use the same path.
    pub gpu_lock: PathBuf,
}
impl SandboxConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.image
                .strip_prefix("sha256:")
                .is_some_and(crate::gpu::valid_digest),
            "sandbox image must be an immutable sha256 ID"
        );
        ensure!(
            self.models.is_absolute()
                && self.models.is_dir()
                && !self.models.to_string_lossy().contains([',', '\n']),
            "invalid model cache path"
        );
        ensure!(self.gpu_lock.is_absolute(), "GPU lock must be absolute");
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Input {
    job: Option<Job>,
    config: WorkerConfig,
}

fn runner_timeout(has_job: bool) -> Duration {
    Duration::from_secs(if has_job { 100 } else { 180 })
}
fn docker_timeout(check_only: bool, has_job: bool) -> Duration {
    if check_only {
        Duration::from_secs(30)
    } else {
        runner_timeout(has_job) + Duration::from_secs(10)
    }
}
pub(crate) fn readiness_timeout() -> Duration {
    docker_timeout(false, false) + Duration::from_secs(10)
}
struct Container(String);
impl Drop for Container {
    fn drop(&mut self) {
        // Container names are generated internally. Synchronous removal also runs when the future is cancelled.
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.0])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
pub fn arguments(name: &str, cfg: &SandboxConfig, cpu: u32, memory: u64, gpu: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "run",
        "--rm",
        "-i",
        "--init",
        "--pull=never",
        "--name",
        name,
        "--network=none",
        "--read-only",
        "--user=65532:65532",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "--pids-limit=128",
        "--stop-timeout=1",
        "--log-driver=none",
        "--ulimit=nofile=1024:1024",
        "--ulimit=core=0:0",
        "--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=134217728,mode=1777",
        "--env=HOME=/tmp",
        "--env=OLLAMA_MODELS=/models",
        "--env=OLLAMA_NO_CLOUD=1",
        "--env=OLLAMA_HOST=127.0.0.1:11434",
        "--env=OLLAMA_NUM_PARALLEL=1",
        "--env=OLLAMA_MAX_LOADED_MODELS=1",
        "--env=OLLAMA_MAX_QUEUE=1",
    ]
    .iter()
    .map(|v| v.to_string())
    .collect();
    args.extend([
        format!("--cpus={cpu}"),
        format!("--memory={memory}m"),
        format!("--memory-swap={memory}m"),
    ]);
    if gpu {
        args.extend([
            "--gpus=all".into(),
            "--env=NVIDIA_DRIVER_CAPABILITIES=compute,utility".into(),
        ]);
    }
    args.extend([
        "--mount".into(),
        format!(
            "type=bind,source={},target=/models,readonly",
            cfg.models.display()
        ),
        "--entrypoint=/usr/local/bin/soapbox".into(),
        cfg.image.clone(),
        "sandbox-run".into(),
    ]);
    args
}
/// A host-wide reservation held before advertising GPU capacity or claiming work.
pub(crate) struct GpuReservation {
    _file: std::fs::File,
    path: PathBuf,
}
pub(crate) fn reserve_gpu(cfg: &SandboxConfig) -> Result<GpuReservation> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&cfg.gpu_lock)?;
    fs2::FileExt::try_lock_exclusive(&file)
        .context("GPU already leased by another Soapbox worker")?;
    Ok(GpuReservation {
        _file: file,
        path: cfg.gpu_lock.clone(),
    })
}
pub async fn execute(job: Option<&Job>, config: &WorkerConfig) -> Result<Value> {
    execute_reserved(job, config, None).await
}
pub(crate) async fn execute_reserved(
    job: Option<&Job>,
    config: &WorkerConfig,
    reservation: Option<&GpuReservation>,
) -> Result<Value> {
    execute_inner(job, config, reservation, false).await
}
/// A CPU-only container verifies the same boundary as a real job without
/// loading a model, acquiring the GPU, or contacting the coordinator.
pub(crate) async fn check(config: &WorkerConfig) -> std::result::Result<(), Failure> {
    match execute_inner(None, config, None, true).await {
        Ok(value) if value == serde_json::json!({"isolation":"docker_v1"}) => Ok(()),
        Ok(_) => Err(Failure::new(
            Code::ProbeFailed,
            "Invalid isolation-check response",
        )),
        Err(error) => {
            let mut failure = error
                .downcast_ref::<Failure>()
                .cloned()
                .unwrap_or_else(|| Failure::new(Code::ProbeFailed, format!("{error:#}")));
            if !failure.blocks_execution() {
                failure.code = Code::ProbeFailed;
            }
            Err(failure)
        }
    }
}
async fn execute_inner(
    job: Option<&Job>,
    config: &WorkerConfig,
    reservation: Option<&GpuReservation>,
    check_only: bool,
) -> Result<Value> {
    let cfg = config.sandbox.as_ref().context("sandbox required")?;
    cfg.validate()?;
    let gpu =
        !check_only && job.is_none_or(|j| matches!(j.spec.workload, Workload::Embeddings { .. }));
    let _lock = if gpu {
        if let Some(held) = reservation {
            ensure!(
                held.path == cfg.gpu_lock,
                "GPU reservation belongs to another device"
            );
            None
        } else {
            Some(reserve_gpu(cfg)?)
        }
    } else {
        None
    };
    let name = format!("soapbox-{}", uuid::Uuid::new_v4());
    let guard = Container(name.clone());
    let (cpu, memory) = job
        .map(|j| (j.spec.resources.cpu_threads, j.spec.resources.memory_mib))
        .unwrap_or((1, config.memory_mib));
    let mut args = arguments(&name, cfg, cpu, memory, gpu);
    if check_only {
        *args.last_mut().expect("runner command") = "sandbox-check".into();
    }
    let mut child = Command::new("docker")
        .args(args)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Failure::new(Code::Unavailable, error.to_string()))?;
    let mut safe_config = config.clone();
    safe_config.sandbox = None;
    safe_config.pause_file = None;
    let input = serde_json::to_vec(&Input {
        job: job.cloned(),
        config: safe_config,
    })?;
    let deadline = docker_timeout(check_only, job.is_some());
    let result = tokio::time::timeout(deadline, async {
        let mut stdin = child.stdin.take().context("runner stdin")?;
        stdin.write_all(&input).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let mut bytes = Vec::new();
        let mut stdout = child
            .stdout
            .take()
            .context("runner stdout")?
            .take(2_097_153);
        let stderr = child.stderr.take().context("runner stderr")?;
        let (_, diagnostic) = tokio::try_join!(
            async {
                stdout.read_to_end(&mut bytes).await?;
                if bytes.len() > 2_097_152 {
                    return Err(anyhow::Error::from(Failure::new(
                        Code::OutputLimit,
                        "stdout exceeded 2 MiB",
                    )));
                }
                Ok(())
            },
            async { Ok::<_, anyhow::Error>(diagnostic_tail(stderr).await?) }
        )?;
        let status = child.wait().await?;
        if !status.success() {
            return Err(Failure::from_stderr(&diagnostic).into());
        }
        Ok::<Value, anyhow::Error>(serde_json::from_slice(&bytes)?)
    })
    .await
    .map_err(|error| Failure::new(Code::Timeout, error.to_string()))?;
    drop(guard);
    result
}
pub async fn runner() -> Result<()> {
    runner_mode(false).await
}
pub async fn check_runner() -> Result<()> {
    runner_mode(true).await
}
async fn runner_mode(check_only: bool) -> Result<()> {
    // Input is still bounded independently. Count it against the original
    // 100-second job deadline; only non-job readiness gets the cold-start budget.
    let started = tokio::time::Instant::now();
    let mut input = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::io::stdin().take(1_048_577).read_to_end(&mut input),
    )
    .await??;
    ensure!(input.len() <= 1_048_576, "runner input limit");
    let request: Input = serde_json::from_slice(&input)?;
    let deadline = started + runner_timeout(check_only || request.job.is_some());
    // In-container deadline survives a worker crash or a disconnected Docker client.
    tokio::time::timeout_at(deadline, async {
        verify_boundary(&request).inspect_err(|error| {
            if let Some(failure) = error.downcast_ref::<Failure>() {
                failure.emit();
            }
        })?;
        if check_only {
            println!("{}", serde_json::json!({"isolation":"docker_v1"}));
            return Ok(());
        }
        if let Some(job) = &request.job {
            job.spec.validate()?;
        }
        let result = if let Some(result) = request
            .job
            .as_ref()
            .and_then(|j| j.spec.workload.cpu_result())
        {
            result
        } else {
            let mut runtime = Command::new("/usr/bin/ollama")
                .arg("serve")
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            let stderr = runtime.stderr.take().context("Ollama stderr")?;
            let (done, stopped) = tokio::sync::oneshot::channel();
            // Drain continuously so a noisy server cannot deadlock on a full pipe.
            // The bounded tail stays in operator-local diagnostics, never the offer.
            let (result, diagnostic) = tokio::join!(
                async {
                    let result = async {
                        wait_for_ollama(
                            &mut runtime,
                            crate::ollama::LOCAL_BACKEND,
                            Duration::from_secs(60),
                        )
                        .await?;
                        if let Some(job) = &request.job {
                            crate::ollama::execute(
                                job,
                                &request.config,
                                crate::ollama::LOCAL_BACKEND,
                            )
                            .await
                        } else {
                            Ok(serde_json::to_value(
                                crate::ollama::ready(crate::ollama::LOCAL_BACKEND, &request.config)
                                    .await?,
                            )?)
                        }
                    }
                    .await;
                    // Always reap the child, including startup failure. The outer
                    // job deadline still bounds startup plus all model work.
                    let _ = runtime.kill().await;
                    let _ = runtime.wait().await;
                    let _ = done.send(());
                    result
                },
                diagnostic_tail_until(stderr, async {
                    let _ = stopped.await;
                })
            );
            result.map_err(|error: anyhow::Error| {
                let tail = diagnostic.unwrap_or_default();
                let failure = Failure::new(
                    Code::RuntimeFailed,
                    format!(
                        "Ollama diagnostic (local only): {}\nOllama failure: {error:#}",
                        String::from_utf8_lossy(&tail)
                    ),
                );
                failure.emit();
                anyhow::Error::from(failure)
            })?
        };
        println!("{}", serde_json::to_string(&result)?);
        Ok::<(), anyhow::Error>(())
    })
    .await?
}

async fn wait_for_ollama(
    runtime: &mut tokio::process::Child,
    backend: &str,
    budget: Duration,
) -> Result<()> {
    // Only the fixed loopback service is used in production. Never consult host
    // proxy settings for sandbox readiness or follow a redirect to another host.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(500))
        .build()?;
    let result = tokio::time::timeout(budget, async {
        loop {
            if let Some(status) = runtime.try_wait()? {
                anyhow::bail!("isolated Ollama exited before readiness ({status})");
            }
            if let Ok(response) = client.get(format!("{backend}/api/version")).send().await {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<Value>().await {
                        if body
                            .get("version")
                            .and_then(Value::as_str)
                            .is_some_and(|v| !v.trim().is_empty())
                        {
                            return Ok(());
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await;
    result.context(format!(
        "isolated Ollama did not become ready within {} seconds; check disk and RAM pressure",
        budget.as_secs()
    ))?
}

fn verify_boundary(request: &Input) -> Result<()> {
    verify_boundary_inner(request).map_err(|error| {
        if error.downcast_ref::<Failure>().is_some() {
            error
        } else {
            Failure::new(Code::BoundaryInvalid, format!("{error:#}")).into()
        }
    })
}
fn verify_boundary_inner(request: &Input) -> Result<()> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    ensure!(
        status
            .lines()
            .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("65532")),
        "sandbox UID missing"
    );
    for expected in ["NoNewPrivs:\t1", "Seccomp:\t2", "CapEff:\t0000000000000000"] {
        ensure!(
            status.lines().any(|l| l == expected),
            "sandbox security boundary missing"
        );
    }
    let memory = std::fs::read_to_string("/sys/fs/cgroup/memory.max")?
        .trim()
        .parse::<u64>()?;
    let swap = std::fs::read_to_string("/sys/fs/cgroup/memory.swap.max")?;
    if swap.trim() != "0" {
        return Err(Failure::new(
            Code::SwapUnenforced,
            "sandbox swap limit is not enforced; fix the Docker daemon cgroup",
        )
        .into());
    }
    let pids = std::fs::read_to_string("/sys/fs/cgroup/pids.max")?
        .trim()
        .parse::<u64>()?;
    let cpu = std::fs::read_to_string("/sys/fs/cgroup/cpu.max")?;
    let cpu: Vec<u64> = cpu
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    let (threads, mib) = request
        .job
        .as_ref()
        .map(|j| (j.spec.resources.cpu_threads, j.spec.resources.memory_mib))
        .unwrap_or((1, request.config.memory_mib));
    ensure!(
        memory <= mib * 1024 * 1024
            && pids <= 128
            && cpu.len() == 2
            && cpu[0] <= cpu[1] * u64::from(threads),
        "cgroup limits are not enforced"
    );
    let mounts = std::fs::read_to_string("/proc/mounts")?;
    ensure!(
        mounts.lines().any(|line| {
            let v: Vec<_> = line.split_whitespace().collect();
            v.len() > 3 && v[1] == "/" && v[3].split(',').any(|x| x == "ro")
        }),
        "root filesystem is writable"
    );
    let network = std::fs::read_to_string("/proc/net/dev")?;
    ensure!(
        !network
            .lines()
            .filter_map(|l| l.split_once(':'))
            .any(|(name, _)| name.trim() != "lo"),
        "network isolation missing"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_probe_budget_does_not_extend_paid_or_private_job_execution() {
        assert_eq!(runner_timeout(true), Duration::from_secs(100));
        assert_eq!(docker_timeout(false, true), Duration::from_secs(110));
        assert_eq!(docker_timeout(true, false), Duration::from_secs(30));
        assert!(runner_timeout(false) > runner_timeout(true));
        assert!(docker_timeout(false, false) > runner_timeout(false));
        assert!(readiness_timeout() > docker_timeout(false, false));
    }

    #[cfg(unix)]
    async fn startup_probe(reply: axum::Router, budget: Duration) -> Result<()> {
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = socket.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(socket, reply).await });
        let mut child = Command::new("sleep").arg("30").kill_on_drop(true).spawn()?;
        let result = wait_for_ollama(&mut child, &format!("http://{address}"), budget).await;
        let _ = child.kill().await;
        let _ = child.wait().await;
        server.abort();
        result
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn readiness_waits_through_http_failure_for_a_valid_version() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let route = axum::Router::new().route(
            "/api/version",
            axum::routing::get(move || {
                let count = count.clone();
                async move {
                    let status = if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        axum::http::StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        axum::http::StatusCode::OK
                    };
                    (status, axum::Json(serde_json::json!({"version":"test"})))
                }
            }),
        );
        startup_probe(route, Duration::from_secs(2)).await.unwrap();
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn readiness_rejects_success_without_a_version() {
        let route = axum::Router::new().route(
            "/api/version",
            axum::routing::get(|| async { axum::Json(serde_json::json!({"version":" "})) }),
        );
        assert!(startup_probe(route, Duration::from_millis(80))
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn readiness_deadline_includes_a_stalled_http_request() {
        let route = axum::Router::new().route(
            "/api/version",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                axum::Json(serde_json::json!({"version":"test"}))
            }),
        );
        // An external bound catches the old retry-count loop, whose network
        // waits could multiply its advertised startup deadline.
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            startup_probe(route, Duration::from_millis(80)),
        )
        .await
        .unwrap();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("did not become ready"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn readiness_reports_early_process_exit() {
        let mut child = Command::new("sh").args(["-c", "exit 7"]).spawn().unwrap();
        child.wait().await.unwrap();
        let error = wait_for_ollama(&mut child, "http://127.0.0.1:1", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("exited before readiness"));
    }

    #[test]
    fn gpu_reservation_excludes_other_workers_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SandboxConfig {
            image: format!("sha256:{}", "a".repeat(64)),
            models: dir.path().into(),
            gpu_lock: dir.path().join("gpu.lock"),
        };
        let first = reserve_gpu(&cfg).unwrap();
        assert!(reserve_gpu(&cfg).is_err());
        drop(first);
        assert!(reserve_gpu(&cfg).is_ok());
    }
    #[test]
    fn sandbox_has_no_host_access_and_requires_immutable_image() {
        let cfg = SandboxConfig {
            image: format!("sha256:{}", "a".repeat(64)),
            models: std::env::temp_dir(),
            gpu_lock: std::env::temp_dir().join("gpu.lock"),
        };
        cfg.validate().unwrap();
        let args = arguments("soapbox-test", &cfg, 2, 512, true);
        for flag in [
            "--network=none",
            "--read-only",
            "--user=65532:65532",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges:true",
            "--memory=512m",
            "--memory-swap=512m",
            "--cpus=2",
            "--pids-limit=128",
            "--gpus=all",
        ] {
            assert!(args.iter().any(|a| a == flag));
        }
        assert!(!args.iter().any(|a| a.contains("docker.sock")
            || a.contains("privileged")
            || a.contains("unconfined")));
        let mut mutable = cfg;
        mutable.image = "ollama:latest".into();
        assert!(mutable.validate().is_err());
    }
}
