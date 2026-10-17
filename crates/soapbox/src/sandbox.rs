//! Operator-owned Docker executor. No buyer images, mounts, environment variables or commands.
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
pub async fn execute(job: Option<&Job>, config: &WorkerConfig) -> Result<Value> {
    let cfg = config.sandbox.as_ref().context("sandbox required")?;
    cfg.validate()?;
    let gpu = job.is_none_or(|j| matches!(j.spec.workload, Workload::Embeddings { .. }));
    let _lock = if gpu {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&cfg.gpu_lock)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("GPU already leased by another Soapbox worker")?;
        Some(file)
    } else {
        None
    };
    let name = format!("soapbox-{}", uuid::Uuid::new_v4());
    let guard = Container(name.clone());
    let (cpu, memory) = job
        .map(|j| (j.spec.resources.cpu_threads, j.spec.resources.memory_mib))
        .unwrap_or((1, config.memory_mib));
    let mut child = Command::new("docker")
        .args(arguments(&name, cfg, cpu, memory, gpu))
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut safe_config = config.clone();
    safe_config.sandbox = None;
    safe_config.pause_file = None;
    let input = serde_json::to_vec(&Input {
        job: job.cloned(),
        config: safe_config,
    })?;
    let result = tokio::time::timeout(Duration::from_secs(110), async {
        let mut stdin = child.stdin.take().context("runner stdin")?;
        stdin.write_all(&input).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let mut bytes = Vec::new();
        let mut diagnostic = Vec::new();
        let mut stdout = child
            .stdout
            .take()
            .context("runner stdout")?
            .take(2_097_153);
        let mut stderr = child.stderr.take().context("runner stderr")?.take(8193);
        tokio::try_join!(
            stdout.read_to_end(&mut bytes),
            stderr.read_to_end(&mut diagnostic)
        )?;
        ensure!(
            bytes.len() <= 2_097_152 && diagnostic.len() <= 8192,
            "sandbox output limit exceeded"
        );
        let status = child.wait().await?;
        ensure!(
            status.success(),
            "isolated workload failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic)
        );
        Ok::<Value, anyhow::Error>(serde_json::from_slice(&bytes)?)
    })
    .await?;
    drop(guard);
    result
}
pub async fn runner() -> Result<()> {
    // In-container deadline survives a worker crash or a disconnected Docker client.
    tokio::time::timeout(Duration::from_secs(100), async {
        let mut input = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::stdin().take(1_048_577).read_to_end(&mut input),
        )
        .await??;
        ensure!(input.len() <= 1_048_576, "runner input limit");
        let request: Input = serde_json::from_slice(&input)?;
        verify_boundary(&request)?;
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
                .stderr(Stdio::null())
                .spawn()?;
            let client = reqwest::Client::new();
            let mut listening = false;
            for _ in 0..300 {
                ensure!(
                    runtime.try_wait()?.is_none(),
                    "isolated Ollama exited before readiness"
                );
                if client
                    .get(format!("{}/api/version", crate::ollama::LOCAL_BACKEND))
                    .timeout(Duration::from_millis(200))
                    .send()
                    .await
                    .is_ok()
                {
                    listening = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            ensure!(
                listening,
                "isolated Ollama did not listen within 30 seconds"
            );
            let output = if let Some(job) = &request.job {
                crate::ollama::execute(job, &request.config, crate::ollama::LOCAL_BACKEND).await?
            } else {
                serde_json::to_value(
                    crate::ollama::ready(crate::ollama::LOCAL_BACKEND, &request.config).await?,
                )?
            };
            runtime.kill().await?;
            let _ = runtime.wait().await;
            output
        };
        println!("{}", serde_json::to_string(&result)?);
        Ok::<(), anyhow::Error>(())
    })
    .await?
}

fn verify_boundary(request: &Input) -> Result<()> {
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
    ensure!(
        swap.trim() == "0",
        "sandbox swap limit is not enforced; fix the Docker daemon cgroup"
    );
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
