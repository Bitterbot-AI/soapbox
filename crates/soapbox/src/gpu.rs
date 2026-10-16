use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gpu {
    pub name: String,
    pub uuid: String,
    pub driver: String,
    pub memory_total_mib: u64,
    pub memory_free_mib: u64,
    pub utilization_pct: u32,
    pub temperature_c: u32,
}
pub async fn inventory() -> Result<Vec<Gpu>> {
    let mut command = tokio::process::Command::new("nvidia-smi");
    command.args(["--query-gpu=name,uuid,driver_version,memory.total,memory.free,utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits"]);
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(3), command.output()).await??;
    ensure!(output.status.success(), "NVIDIA inventory unavailable");
    parse_inventory(&String::from_utf8(output.stdout)?)
}
fn parse_inventory(text: &str) -> Result<Vec<Gpu>> {
    let mut devices = Vec::new();
    for line in text.lines().take(16) {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        ensure!(fields.len() == 7, "invalid NVIDIA inventory");
        devices.push(Gpu {
            name: fields[0].into(),
            uuid: fields[1].into(),
            driver: fields[2].into(),
            memory_total_mib: fields[3].parse()?,
            memory_free_mib: fields[4].parse()?,
            utilization_pct: fields[5].parse()?,
            temperature_c: fields[6].parse()?,
        });
    }
    ensure!(!devices.is_empty(), "no NVIDIA GPU found");
    Ok(devices)
}
pub fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub model: String,
    pub model_digest: String,
    pub version: String,
    pub model_bytes: u64,
    pub gpu_bytes: u64,
    pub observed_at: i64,
}
impl Runtime {
    pub fn fully_gpu(&self) -> bool {
        self.model_bytes > 0 && self.gpu_bytes >= self.model_bytes
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub backend: String,
    pub model: String,
    pub model_digest: String,
    pub runtime_version: String,
    pub fully_gpu_resident: bool,
    pub model_bytes: u64,
    pub gpu_bytes: u64,
    pub dimensions: usize,
    pub batches: usize,
    pub wall_ms: u64,
    pub total_duration_ns: u64,
    pub load_duration_ns: u64,
    pub prompt_tokens: u64,
    pub hardware: Vec<Gpu>,
}
impl Execution {
    pub fn read(value: &Value) -> Result<Self> {
        Ok(serde_json::from_value(
            value
                .get("execution")
                .context("missing execution evidence")?
                .clone(),
        )?)
    }
    pub fn validate(&self, model: &str, digest: &str, dimensions: usize) -> Result<()> {
        ensure!(
            self.backend == "ollama" && self.model == model && self.model_digest == digest,
            "runtime/model binding mismatch"
        );
        ensure!(
            !self.runtime_version.is_empty() && self.runtime_version.len() <= 128,
            "invalid runtime version"
        );
        ensure!(
            self.dimensions == dimensions && (1..=128).contains(&self.batches),
            "invalid execution dimensions/batches"
        );
        ensure!(
            self.wall_ms <= 110_000
                && self.total_duration_ns <= 110_000_000_000
                && self.load_duration_ns <= self.total_duration_ns,
            "invalid duration evidence"
        );
        ensure!(self.hardware.len() <= 16, "invalid hardware evidence");
        if self.fully_gpu_resident {
            ensure!(
                self.model_bytes > 0
                    && self.gpu_bytes >= self.model_bytes
                    && !self.hardware.is_empty(),
                "GPU execution evidence incomplete"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_parser_fails_closed_for_missing_metrics() {
        let gpus = parse_inventory("RTX 4080, GPU-test, 610.88, 16376, 14000, 4, 50\n").unwrap();
        assert_eq!(gpus[0].memory_free_mib, 14000);
        assert!(parse_inventory("RTX, UUID, 610, 16000, 12000, [N/A], 50").is_err());
        assert!(parse_inventory("").is_err());
        assert!(!valid_digest(&"A".repeat(64)));
    }
}
