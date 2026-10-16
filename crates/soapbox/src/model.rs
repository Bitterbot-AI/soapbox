use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const API_VERSION: u32 = 1;
pub const LEASE_SECONDS: i64 = 30;
pub const MAX_RUN_SECONDS: i64 = 120;
pub const MAX_ATTEMPTS: u32 = 3;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn digest(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mode {
    Paid { max_cost_credits: u64 },
    Private { pool_id: String },
    Collective { project_id: String },
}
impl Mode {
    pub fn name(&self) -> &str {
        match self {
            Self::Paid { .. } => "paid",
            Self::Private { .. } => "private",
            Self::Collective { .. } => "collective",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Workload {
    Sha256 { texts: Vec<String> },
    Embeddings { model: String, texts: Vec<String> },
}
impl Workload {
    pub fn texts(&self) -> &[String] {
        match self {
            Self::Sha256 { texts } | Self::Embeddings { texts, .. } => texts,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.texts().is_empty() && self.texts().len() <= 128,
            "expected 1..128 inputs"
        );
        ensure!(
            self.texts().iter().map(|s| s.len()).sum::<usize>() <= 262_144,
            "inputs exceed 256 KiB"
        );
        if let Self::Embeddings { model, .. } = self {
            ensure!(!model.is_empty() && model.len() <= 128, "invalid model");
        }
        Ok(())
    }
    pub fn price(&self) -> u64 {
        self.texts().len() as u64
    }
    pub fn cpu_result(&self) -> Option<Value> {
        match self {
            Self::Sha256 { texts } => Some(
                json!({"hashes": texts.iter().map(|s| digest(s.as_bytes())).collect::<Vec<_>>()}),
            ),
            _ => None,
        }
    }
    pub fn verify(&self, result: &Value) -> Result<&'static str> {
        if let Some(expected) = self.cpu_result() {
            ensure!(result == &expected, "incorrect SHA-256 result");
            return Ok("recomputed");
        }
        let rows = result
            .get("embeddings")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("missing embeddings"))?;
        ensure!(rows.len() == self.texts().len(), "embedding count mismatch");
        let mut dimensions = None;
        for row in rows {
            let values = row
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid embedding"))?;
            ensure!(
                !values.is_empty() && values.len() <= 8192,
                "invalid dimensions"
            );
            ensure!(
                values
                    .iter()
                    .all(|v| v.as_f64().is_some_and(f64::is_finite)),
                "non-finite embedding"
            );
            ensure!(
                dimensions.is_none_or(|d| d == values.len()),
                "inconsistent dimensions"
            );
            dimensions = Some(values.len());
        }
        Ok("shape_only_trusted_worker")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub cpu_threads: u32,
    pub memory_mib: u64,
    #[serde(default)]
    pub require_gpu: bool,
}
impl Default for Resources {
    fn default() -> Self {
        Self {
            cpu_threads: 1,
            memory_mib: 256,
            require_gpu: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    pub idempotency_key: String,
    pub mode: Mode,
    pub workload: Workload,
    #[serde(default)]
    pub resources: Resources,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub cpu_threads: u32,
    pub memory_mib: u64,
    pub gpu_observed: bool,
    pub models: Vec<String>,
    pub modes: Vec<String>,
    pub pools: Vec<String>,
    pub projects: Vec<String>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub min_price_credits: u64,
}
impl Offer {
    pub fn accepts(&self, spec: &Submit) -> bool {
        if self.paused
            || self.cpu_threads < spec.resources.cpu_threads
            || self.memory_mib < spec.resources.memory_mib
            || (spec.resources.require_gpu && !self.gpu_observed)
            || !self.modes.iter().any(|m| m == spec.mode.name())
        {
            return false;
        }
        if let Workload::Embeddings { model, .. } = &spec.workload {
            if !self.models.contains(model) {
                return false;
            }
        } else if spec.resources.require_gpu {
            return false;
        }
        match &spec.mode {
            Mode::Paid { .. } => spec.workload.price() >= self.min_price_credits,
            Mode::Private { pool_id } => self.pools.contains(pool_id),
            Mode::Collective { project_id } => self.projects.contains(project_id),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub owner: String,
    pub spec: Submit,
    pub status: String,
    pub attempts: u32,
    pub created_at: i64,
    pub deadline: i64,
    pub worker: Option<String>,
    pub lease_id: Option<String>,
    pub lease_until: Option<i64>,
    pub run_started_at: Option<i64>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub receipt: Option<Value>,
    pub reserved_credits: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub lease_id: String,
    pub result: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub offer: Offer,
    pub last_seen: i64,
}
