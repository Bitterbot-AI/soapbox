//! Chain-verified escrow accounting. Signing keys never enter the coordinator or worker.
use crate::{
    api::{ApiError, App},
    config::Principal,
    error::UserFacing,
    model::*,
};
use anyhow::{ensure, Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentConfig {
    pub rpc_url: String,
    pub chain_id: u64,
    pub token: String,
    pub escrow: String,
    pub escrow_code_sha256: String,
    pub microusdc_per_item: u64,
    pub confirmations: u64,
    pub wallets: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payment {
    pub chain_id: u64,
    pub token: String,
    pub escrow: String,
    pub key: String,
    pub buyer: String,
    pub operator: String,
    pub operator_id: String,
    pub amount_microusdc: u64,
    pub expires: i64,
    #[serde(default)]
    pub confirmed_block: Option<String>,
    #[serde(default)]
    pub confirmed_block_hash: Option<String>,
}
pub fn address(v: &str) -> bool {
    v.len() == 42
        && v.starts_with("0x")
        && v[2..].bytes().all(|c| c.is_ascii_hexdigit())
        && v[2..].bytes().any(|c| c != b'0')
}
impl PaymentConfig {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.rpc_url)?;
        ensure!(
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
            "invalid RPC URL"
        );
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))),
            "RPC requires HTTPS or loopback"
        );
        // Public mainnet deliberately remains disabled until the pilot and contract review finish.
        ensure!(
            [31337, 84532].contains(&self.chain_id),
            "only local EVM or Base Sepolia is enabled"
        );
        ensure!(
            address(&self.token)
                && address(&self.escrow)
                && self.wallets.values().all(|v| address(v)),
            "invalid payment address"
        );
        ensure!(
            crate::gpu::valid_digest(&self.escrow_code_sha256),
            "pin deployed escrow bytecode SHA-256"
        );
        ensure!(
            (1..=1_000_000).contains(&self.microusdc_per_item)
                && (1..=64).contains(&self.confirmations),
            "invalid pricing or confirmations"
        );
        if self.chain_id == 84532 {
            ensure!(
                self.token
                    .eq_ignore_ascii_case("0x036CbD53842c5426634e7929541eC2318f3dCF7e"),
                "Base Sepolia requires Circle USDC"
            );
            ensure!(
                self.confirmations >= 2,
                "testnet requires at least two confirmations"
            );
        }
        Ok(())
    }
    pub fn quote(
        &self,
        id: &str,
        owner: &str,
        operator: &str,
        spec: &Submit,
        budget: u64,
        at: i64,
    ) -> Result<Payment> {
        self.validate()?;
        let amount = self
            .microusdc_per_item
            .checked_mul(spec.workload.price())
            .context("price overflow")?;
        ensure!(
            amount <= budget && budget <= 100_000_000,
            "USDC budget below quote or above pilot cap"
        );
        let buyer = self
            .wallets
            .get(owner)
            .context("buyer wallet is not provisioned")?
            .to_lowercase();
        let payee = self
            .wallets
            .get(operator)
            .context("operator wallet is not provisioned")?
            .to_lowercase();
        ensure!(
            owner != operator && buyer != payee,
            "buyer and operator must be distinct"
        );
        let key = format!(
            "0x{}",
            digest(&serde_json::to_vec(&(
                id,
                owner,
                spec,
                self.chain_id,
                &self.escrow
            ))?)
        );
        Ok(Payment {
            chain_id: self.chain_id,
            token: self.token.clone(),
            escrow: self.escrow.clone(),
            key,
            buyer,
            operator: payee,
            operator_id: operator.into(),
            amount_microusdc: amount,
            expires: at + 3600,
            confirmed_block: None,
            confirmed_block_hash: None,
        })
    }
    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut response = client
            .post(&self.rpc_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(reqwest::Error::without_url)?;
        let mut data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            ensure!(
                data.len() + chunk.len() <= 2_097_152,
                "RPC response too large"
            );
            data.extend_from_slice(&chunk);
        }
        let v: Value = serde_json::from_slice(&data)?;
        ensure!(v.get("error").is_none(), "chain RPC rejected request");
        v.get("result").cloned().context("missing RPC result")
    }
    async fn call(&self, data: String, block: &str) -> Result<String> {
        self.rpc("eth_call", json!([{"to":self.escrow,"data":data}, block]))
            .await?
            .as_str()
            .map(str::to_string)
            .context("invalid contract result")
    }
    pub async fn observe(&self, p: &Payment) -> Result<Observation> {
        self.validate()?;
        ensure!(
            p.chain_id == self.chain_id
                && p.escrow.eq_ignore_ascii_case(&self.escrow)
                && p.token.eq_ignore_ascii_case(&self.token),
            "payment configuration changed"
        );
        let chain = self.rpc("eth_chainId", json!([])).await?;
        ensure!(
            number(chain.as_str().context("chain ID")?)? == self.chain_id,
            "wrong RPC chain"
        );
        let head = self.rpc("eth_blockNumber", json!([])).await?;
        let block = format!(
            "0x{:x}",
            number(head.as_str().context("block")?)?.saturating_sub(self.confirmations - 1)
        );
        let info = self
            .rpc("eth_getBlockByNumber", json!([block, false]))
            .await?;
        let hash = info["hash"]
            .as_str()
            .context("missing block hash")?
            .to_string();
        let code = self.rpc("eth_getCode", json!([self.escrow, block])).await?;
        ensure!(
            digest(&hex::decode(
                code.as_str().context("code")?.trim_start_matches("0x")
            )?) == self.escrow_code_sha256,
            "escrow bytecode mismatch"
        );
        let token = self.call(selector("token()"), &block).await?;
        ensure!(
            token.len() == 66 && token[26..].eq_ignore_ascii_case(&self.token[2..]),
            "escrow token mismatch"
        );
        let data = self
            .call(
                format!("{}{}", selector("jobs(bytes32)"), &p.key[2..]),
                &block,
            )
            .await?;
        let words: Vec<&str> = data
            .strip_prefix("0x")
            .context("ABI prefix")?
            .as_bytes()
            .chunks(64)
            .map(std::str::from_utf8)
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            words.len() == 6 && words.iter().all(|w| w.len() == 64),
            "invalid escrow ABI"
        );
        let state = number(words[4])?;
        if state != 0 && state != 4 {
            ensure!(
                words[0][24..].eq_ignore_ascii_case(&p.buyer[2..])
                    && words[1][24..].eq_ignore_ascii_case(&p.operator[2..])
                    && number(words[2])? == p.amount_microusdc
                    && number(words[3])? == p.expires as u64,
                "funding does not match quote"
            );
        }
        let after = self
            .rpc("eth_getBlockByNumber", json!([block, false]))
            .await?;
        ensure!(
            after["hash"].as_str() == Some(hash.as_str()),
            "chain changed while checking payment"
        );
        Ok(Observation {
            state,
            result: format!("0x{}", words[5]),
            block,
            hash,
        })
    }
}
fn number(s: &str) -> Result<u64> {
    Ok(u64::from_str_radix(s.trim_start_matches("0x"), 16)?)
}
fn selector(s: &str) -> String {
    format!("0x{}", hex::encode(&Keccak256::digest(s.as_bytes())[..4]))
}
pub struct Observation {
    pub state: u64,
    pub result: String,
    pub block: String,
    pub hash: String,
}

impl crate::store::Store {
    pub fn payment_jobs(&self) -> Result<Vec<Job>> {
        let mut stmt = self.db.prepare("SELECT data FROM jobs WHERE status IN ('awaiting_payment','queued','running','settlement_pending','refund_pending','payment_review') ORDER BY rowid LIMIT 256")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut jobs = Vec::new();
        for row in rows {
            let job: Job = serde_json::from_str(&row?)?;
            if job.payment.is_some() {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }
    pub fn apply_payment(&mut self, id: &str, observation: Observation) -> Result<Job> {
        let tx = self.db.transaction()?;
        let data: String = tx.query_row("SELECT data FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        let mut job: Job = serde_json::from_str(&data)?;
        let payment = job.payment.as_mut().context("not a USDC job")?;
        match observation.state {
            0 => {
                if ["queued", "running", "settlement_pending"].contains(&job.status.as_str()) {
                    job.status = "payment_review".into();
                    job.lease_id = None;
                    job.lease_until = None;
                    job.error = Some("escrow funding disappeared; awaiting reconciliation".into());
                }
            }
            1 => {
                if job.status == "payment_review" && job.result.is_some() {
                    job.status = "settlement_pending".into();
                } else if job.status == "awaiting_payment" || job.status == "payment_review" {
                    if job.deadline > now() {
                        job.status = "queued".into();
                    } else {
                        job.status = "refund_pending".into();
                    }
                }
            }
            2 => {
                ensure!(
                    job.status == "settlement_pending" || job.status == "succeeded",
                    "unexpected escrow settlement"
                );
                let receipt = job.receipt.as_mut().context("missing completion receipt")?;
                ensure!(
                    observation.result
                        == format!(
                            "0x{}",
                            receipt["result_sha256"]
                                .as_str()
                                .context("missing result digest")?
                        ),
                    "settlement result mismatch"
                );
                receipt["payment_state"] = json!("paid");
                receipt["chain_id"] = json!(payment.chain_id);
                receipt["escrow"] = json!(payment.escrow);
                receipt["operator_address"] = json!(payment.operator);
                receipt["confirmed_block_hash"] = json!(observation.hash);
                job.status = "succeeded".into();
            }
            3 => {
                ensure!(job.status != "succeeded", "paid job cannot be refunded");
                job.status = "refunded".into();
                job.error = Some("escrow refunded to buyer".into());
            }
            4 => {
                ensure!(
                    job.status == "refund_pending" || job.status == "cancelled",
                    "unexpected voided escrow"
                );
                job.status = "cancelled".into();
            }
            _ => anyhow::bail!("unknown escrow state"),
        }
        payment.confirmed_block = Some(observation.block);
        payment.confirmed_block_hash = Some(observation.hash);
        crate::store::save(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
}
pub async fn refresh(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Job>, ApiError> {
    let (job, cfg) = app.with(|s| {
        let job = s.job(&id)?;
        if job.owner != p.id && !p.has("admin") {
            return Err(UserFacing::forbidden("job not accessible").into());
        }
        Ok((job, s.payments.clone().context("USDC disabled")?))
    })?;
    let payment = job
        .payment
        .as_ref()
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "not a USDC job".into()))?;
    let observation = cfg.observe(payment).await.map_err(|e| {
        ApiError::opaque(
            &e,
            StatusCode::SERVICE_UNAVAILABLE,
            "payment verification unavailable",
        )
    })?;
    app.with(|s| Ok(Json(s.apply_payment(&id, observation)?)))
}
pub async fn intents(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
) -> std::result::Result<Json<Vec<Value>>, ApiError> {
    app.with(|s| {
        if !p.has("admin") {
            return Err(UserFacing::forbidden("admin role required").into());
        }
        Ok(Json(s.payment_jobs()?.into_iter().map(|j| json!({"id":j.id,"status":j.status,"payment":j.payment,"result_sha256":j.receipt.and_then(|r|r.get("result_sha256").cloned())})).collect()))
    })
}
