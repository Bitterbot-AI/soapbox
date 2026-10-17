use crate::model::{Mode, Offer};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: String,
    pub token_env: String,
    pub roles: Vec<String>,
    #[serde(default)]
    pub pools: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
}
impl Principal {
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
    pub fn permits(&self, mode: &Mode) -> bool {
        match mode {
            Mode::Paid { .. } | Mode::PaidUsdc { .. } => true,
            Mode::Private { pool_id } => self.pools.contains(pool_id),
            Mode::Collective { project_id } => self.projects.contains(project_id),
        }
    }
    pub fn restrict_offer(&self, offer: &mut Offer) {
        offer.pools.retain(|p| self.pools.contains(p));
        offer.projects.retain(|p| self.projects.contains(p));
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub payments: Option<crate::payment::PaymentConfig>,
    pub principals: Vec<Principal>,
    #[serde(default = "quota")]
    pub collective_daily_items: u64,
    #[serde(default = "storage_quota")]
    pub artifact_quota_bytes: u64,
}
fn quota() -> u64 {
    1000
}
fn storage_quota() -> u64 {
    16 * 1024 * 1024
}
impl Config {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            !config.principals.is_empty(),
            "at least one principal required"
        );
        if let Some(payment) = &config.payments {
            payment.validate()?;
        }
        let mut ids = HashSet::new();
        let mut tokens = HashSet::new();
        for p in &config.principals {
            ensure!(
                !p.id.is_empty() && p.id.len() <= 128 && ids.insert(&p.id),
                "invalid or duplicate principal id"
            );
            ensure!(
                p.roles
                    .iter()
                    .all(|r| ["admin", "submitter", "worker"].contains(&r.as_str())),
                "unknown role"
            );
            let token = std::env::var(&p.token_env)
                .with_context(|| format!("missing token variable {}", p.token_env))?;
            ensure!(
                token.len() >= 32 && tokens.insert(token),
                "tokens must be unique and at least 32 characters"
            );
        }
        Ok(config)
    }
}
