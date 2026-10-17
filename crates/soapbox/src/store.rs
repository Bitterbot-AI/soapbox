use crate::{config::Principal, model::*};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use uuid::Uuid;

pub struct Store {
    pub payments: Option<crate::payment::PaymentConfig>,
    pub db: Connection,
    pub collective_daily_items: u64,
    pub artifact_quota_bytes: u64,
}
impl Store {
    pub fn open(path: &std::path::Path, daily_items: u64, artifact_bytes: u64) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS meta(version INTEGER NOT NULL);
            INSERT INTO meta SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM meta);
            CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, owner TEXT NOT NULL, idem TEXT NOT NULL,
                status TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(owner,idem));
            CREATE TABLE IF NOT EXISTS nodes(id TEXT PRIMARY KEY, data TEXT NOT NULL, paused INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, balance INTEGER NOT NULL DEFAULT 0 CHECK(balance>=0),
                reserved INTEGER NOT NULL DEFAULT 0 CHECK(reserved>=0));
            CREATE TABLE IF NOT EXISTS ledger(ref TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL,
                amount INTEGER NOT NULL, job TEXT, at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS contributions(owner TEXT NOT NULL, project TEXT NOT NULL, day INTEGER NOT NULL,
                items INTEGER NOT NULL, PRIMARY KEY(owner,project,day));
            CREATE TABLE IF NOT EXISTS artifacts(id TEXT PRIMARY KEY, owner TEXT NOT NULL, content TEXT NOT NULL,
                digest TEXT NOT NULL, expires_at INTEGER NOT NULL, bytes INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS jobs_owner ON jobs(owner);
            CREATE INDEX IF NOT EXISTS jobs_status ON jobs(status);
            CREATE INDEX IF NOT EXISTS artifacts_owner ON artifacts(owner);
        ")?;
        let version: i64 = db.query_row("SELECT version FROM meta", [], |r| r.get(0))?;
        ensure!(version == 1, "unsupported database schema");
        Ok(Self {
            payments: None,
            db,
            collective_daily_items: daily_items,
            artifact_quota_bytes: artifact_bytes,
        })
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        get(&self.db, id)
    }
    pub fn jobs(&self, p: &Principal) -> Result<Vec<Job>> {
        let mut stmt = self
            .db
            .prepare("SELECT data FROM jobs WHERE owner=?1 OR ?2 OR (?3 AND json_extract(data,'$.worker')=?1) ORDER BY rowid DESC LIMIT 100")?;
        let rows = stmt.query_map(params![p.id, p.has("admin"), p.has("worker")], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|r| {
            let mut job: Job = serde_json::from_str(&r?)?;
            job.result = None;
            Ok(job)
        })
        .collect()
    }
    pub fn balance(&self, id: &str) -> Result<Value> {
        let row: Option<(i64, i64)> = self
            .db
            .query_row(
                "SELECT balance,reserved FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (balance, reserved) = row.unwrap_or((0, 0));
        Ok(
            json!({"account":id,"available_credits":balance,"reserved_credits":reserved,"currency":"TEST_CREDIT","redeemable":false}),
        )
    }
    pub fn credit(&mut self, id: &str, amount: u64, key: &str, at: i64) -> Result<Value> {
        ensure!(
            !id.is_empty() && id.len() <= 128 && (1..=1_000_000).contains(&amount),
            "invalid credit"
        );
        ensure!(
            !key.is_empty() && key.len() <= 128,
            "invalid idempotency key"
        );
        let tx = self.db.transaction()?;
        let reference = format!("credit:{key}");
        let old: Option<(String, u64)> = tx
            .query_row(
                "SELECT account,amount FROM ledger WHERE ref=?1",
                [&reference],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((account, previous)) = old {
            ensure!(
                account == id && previous == amount,
                "idempotency key conflict"
            );
        } else {
            account(&tx, id)?;
            let balance: u64 =
                tx.query_row("SELECT balance FROM accounts WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            ensure!(
                balance + amount <= 9_000_000_000_000,
                "account limit exceeded"
            );
            tx.execute(
                "UPDATE accounts SET balance=balance+?1 WHERE id=?2",
                params![amount, id],
            )?;
            ledger(&tx, &reference, id, "test_credit", amount, None, at)?;
        }
        tx.commit()?;
        self.balance(id)
    }
    pub fn submit(&mut self, p: &Principal, spec: Submit, at: i64) -> Result<Job> {
        ensure!(p.has("submitter"), "submitter role required");
        ensure!(p.permits(&spec.mode), "pool/project membership required");
        spec.validate()?;
        ensure!(
            !spec.idempotency_key.is_empty() && spec.idempotency_key.len() <= 128,
            "invalid idempotency key"
        );
        ensure!(
            (1..=64).contains(&spec.resources.cpu_threads)
                && (64..=1_048_576).contains(&spec.resources.memory_mib),
            "invalid resource request"
        );
        if matches!(spec.workload, Workload::Sha256 { .. }) {
            ensure!(!spec.resources.require_gpu, "SHA-256 is a CPU workload");
        }
        let tx = self.db.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM jobs WHERE owner=?1 AND idem=?2",
                params![p.id, spec.idempotency_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let job: Job = serde_json::from_str(&existing)?;
            ensure!(job.spec == spec, "idempotency key conflict");
            return Ok(job);
        }
        let active: u64 = tx.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status IN ('queued','running','awaiting_payment')",
            [],
            |r| r.get(0),
        )?;
        ensure!(active < 256, "queue full");
        let mut job = Job {
            payment: None,
            id: Uuid::new_v4().to_string(),
            owner: p.id.clone(),
            spec,
            status: "queued".into(),
            attempts: 0,
            created_at: at,
            deadline: at + 600,
            worker: None,
            lease_id: None,
            lease_until: None,
            run_started_at: None,
            result: None,
            error: None,
            receipt: None,
            reserved_credits: 0,
        };
        match &job.spec.mode {
            Mode::Paid { max_cost_credits } => {
                let price = job.spec.workload.price();
                ensure!(
                    *max_cost_credits >= price && *max_cost_credits <= 1_000_000,
                    "budget below quote or above limit"
                );
                account(&tx, &p.id)?;
                ensure!(tx.execute("UPDATE accounts SET balance=balance-?1,reserved=reserved+?1 WHERE id=?2 AND balance>=?1",
                    params![price,p.id])?==1,"insufficient test credits");
                job.reserved_credits = price;
                ledger(
                    &tx,
                    &format!("reserve:{}", job.id),
                    &p.id,
                    "reserve",
                    price,
                    Some(&job.id),
                    at,
                )?;
            }
            Mode::Collective { project_id } => {
                let used: u64 = tx
                    .query_row(
                        "SELECT items FROM contributions WHERE owner=?1 AND project=?2 AND day=?3",
                        params![p.id, project_id, at / 86400],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or(0);
                let items = job.spec.workload.price();
                ensure!(
                    used + items <= self.collective_daily_items,
                    "collective daily quota exhausted"
                );
                tx.execute(
                    "INSERT INTO contributions VALUES(?1,?2,?3,?4)
                    ON CONFLICT(owner,project,day) DO UPDATE SET items=items+excluded.items",
                    params![p.id, project_id, at / 86400, items],
                )?;
            }
            Mode::PaidUsdc {
                max_cost_microusdc,
                operator,
            } => {
                let cfg = self
                    .payments
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("USDC settlement is disabled"))?;
                job.payment = Some(cfg.quote(
                    &job.id,
                    &job.owner,
                    operator,
                    &job.spec,
                    *max_cost_microusdc,
                    at,
                )?);
                job.status = "awaiting_payment".into();
            }
            Mode::Private { .. } => {}
        }
        tx.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,?5)",
            params![
                job.id,
                job.owner,
                job.spec.idempotency_key,
                job.status,
                serde_json::to_string(&job)?
            ],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn register(&mut self, p: &Principal, mut offer: Offer, at: i64) -> Result<Node> {
        ensure!(p.has("worker"), "worker role required");
        ensure!(
            (1..=1024).contains(&offer.cpu_threads)
                && (64..=16_777_216).contains(&offer.memory_mib),
            "invalid capacity"
        );
        ensure!(
            offer.models.len() <= 16 && offer.pools.len() <= 64 && offer.projects.len() <= 64,
            "offer too large"
        );
        ensure!(
            offer.modes.len() <= 3
                && offer
                    .modes
                    .iter()
                    .all(|m| ["paid", "private", "collective"].contains(&m.as_str())),
            "invalid modes"
        );
        p.restrict_offer(&mut offer);
        let node = Node {
            id: p.id.clone(),
            offer,
            last_seen: at,
        };
        self.db.execute("INSERT INTO nodes(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![p.id,serde_json::to_string(&node)?])?;
        Ok(node)
    }
    pub fn nodes(&self, p: &Principal, at: i64) -> Result<Vec<Value>> {
        let mut stmt = self.db.prepare("SELECT data,paused FROM nodes")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?;
        let mut nodes = Vec::new();
        for row in rows {
            let (data, paused) = row?;
            let mut node: Node = serde_json::from_str(&data)?;
            if !p.has("admin") && node.id != p.id {
                p.restrict_offer(&mut node.offer);
                if !node.offer.modes.iter().any(|m| m == "paid")
                    && node.offer.pools.is_empty()
                    && node.offer.projects.is_empty()
                {
                    continue;
                }
            }
            let online = at - node.last_seen < 30;
            nodes.push(
                json!({"id":node.id,"offer":node.offer,"last_seen":node.last_seen,
                "online":online,"operator_paused":paused,"inventory_assurance":"self_reported"}),
            );
        }
        Ok(nodes)
    }
    pub fn pause(&mut self, id: &str, paused: bool) -> Result<()> {
        ensure!(
            self.db.execute(
                "UPDATE nodes SET paused=?1 WHERE id=?2",
                params![paused, id]
            )? == 1,
            "unknown worker"
        );
        Ok(())
    }
    pub fn claim(&mut self, p: &Principal, at: i64) -> Result<Option<Job>> {
        ensure!(p.has("worker"), "worker role required");
        self.sweep(at)?;
        let tx = self.db.transaction()?;
        let (data, paused): (String, bool) =
            tx.query_row("SELECT data,paused FROM nodes WHERE id=?1", [&p.id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let node: Node = serde_json::from_str(&data)?;
        if paused || at - node.last_seen >= 30 {
            return Ok(None);
        }
        let running = all_active(&tx)?;
        if running
            .iter()
            .any(|j| j.status == "running" && j.worker.as_ref() == Some(&p.id))
        {
            return Ok(None);
        }
        for mut job in running {
            if job.status != "queued"
                || !p.permits(&job.spec.mode)
                || !node.offer.accepts(&job.spec)
                || job
                    .payment
                    .as_ref()
                    .is_some_and(|pay| pay.operator_id != p.id)
            {
                continue;
            }
            job.status = "running".into();
            job.attempts += 1;
            job.worker = Some(p.id.clone());
            job.lease_id = Some(Uuid::new_v4().to_string());
            job.lease_until = Some((at + LEASE_SECONDS).min(job.deadline));
            job.run_started_at = Some(at);
            job.error = None;
            save(&tx, &job)?;
            tx.commit()?;
            return Ok(Some(job));
        }
        Ok(None)
    }
    pub fn renew(&mut self, p: &Principal, id: &str, lease: &str, at: i64) -> Result<Job> {
        let tx = self.db.transaction()?;
        let mut job = get(&tx, id)?;
        check_lease(&job, p, lease, at)?;
        job.lease_until = Some(
            (at + LEASE_SECONDS)
                .min(job.deadline)
                .min(job.run_started_at.unwrap_or(at) + MAX_RUN_SECONDS),
        );
        save(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
    pub fn complete(
        &mut self,
        p: &Principal,
        id: &str,
        completion: Completion,
        at: i64,
    ) -> Result<Job> {
        let tx = self.db.transaction()?;
        let mut job = get(&tx, id)?;
        if (job.status == "succeeded" || job.status == "settlement_pending")
            && job.worker.as_ref() == Some(&p.id)
            && job.lease_id.as_ref() == Some(&completion.lease_id)
        {
            ensure!(
                job.result.as_ref() == Some(&completion.result),
                "completion replay differs"
            );
            return Ok(job);
        }
        check_lease(&job, p, &completion.lease_id, at)?;
        let assurance = job.spec.verify_result(&completion.result)?;
        ensure!(
            serde_json::to_vec(&completion.result)?.len() <= 2_097_152,
            "result too large"
        );
        let price = job.reserved_credits;
        if price > 0 {
            account(&tx, &p.id)?;
            tx.execute(
                "UPDATE accounts SET reserved=reserved-?1 WHERE id=?2",
                params![price, job.owner],
            )?;
            tx.execute(
                "UPDATE accounts SET balance=balance+?1 WHERE id=?2",
                params![price, p.id],
            )?;
            ledger(
                &tx,
                &format!("settle:{}", job.id),
                &p.id,
                "earned",
                price,
                Some(&job.id),
                at,
            )?;
            job.reserved_credits = 0;
        }
        job.status = "succeeded".into();
        job.receipt = Some(
            json!({"version":1,"job_id":job.id,"worker":p.id,"mode":job.spec.mode.name(),
            "currency":"TEST_CREDIT","charged_credits":price,"verification":assurance,"completed_at":at,
            "elapsed_seconds":at-job.run_started_at.unwrap_or(at),"result_sha256":digest(&serde_json::to_vec(&completion.result)?),
            "execution":completion.result.get("execution"),"execution_assurance":"worker_reported","signature":null,"authority":"coordinator","redeemable":false}),
        );
        if let Some(payment) = &job.payment {
            job.status = "settlement_pending".into();
            let receipt = job.receipt.as_mut().unwrap();
            receipt["currency"] = json!("USDC");
            receipt["charged_credits"] = Value::Null;
            receipt["amount_microusdc"] = json!(payment.amount_microusdc);
            receipt["payment_state"] = json!("pending");
        }
        job.result = Some(completion.result);
        save(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
    pub fn fail(&mut self, p: &Principal, id: &str, lease: &str, at: i64) -> Result<Job> {
        let tx = self.db.transaction()?;
        let mut job = get(&tx, id)?;
        check_lease(&job, p, lease, at)?;
        retry_or_fail(&tx, &mut job, at, "worker execution failed")?;
        save(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
    pub fn cancel(&mut self, p: &Principal, id: &str, at: i64) -> Result<Job> {
        let tx = self.db.transaction()?;
        let mut job = get(&tx, id)?;
        ensure!(p.id == job.owner || p.has("admin"), "job not accessible");
        if ["queued", "running", "awaiting_payment"].contains(&job.status.as_str()) {
            job.status = "cancelled".into();
            refund(&tx, &mut job, at)?;
            save(&tx, &job)?;
        }
        tx.commit()?;
        Ok(job)
    }
    pub fn sweep(&mut self, at: i64) -> Result<()> {
        let tx = self.db.transaction()?;
        for mut job in all_active(&tx)? {
            if job.deadline <= at
                || (job.status == "running" && job.lease_until.is_none_or(|t| t <= at))
            {
                retry_or_fail(&tx, &mut job, at, "lease or job deadline expired")?;
                save(&tx, &job)?;
            }
        }
        tx.execute("DELETE FROM artifacts WHERE expires_at<=?1", [at])?;
        tx.commit()?;
        Ok(())
    }
    pub fn artifact_put(
        &mut self,
        p: &Principal,
        content: &str,
        ttl: i64,
        at: i64,
    ) -> Result<Value> {
        ensure!(p.has("submitter"), "submitter role required");
        ensure!(
            !content.is_empty() && content.len() <= 1_048_576 && (60..=86400).contains(&ttl),
            "invalid artifact or TTL"
        );
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM artifacts WHERE expires_at<=?1", [at])?;
        let used: u64 = tx.query_row(
            "SELECT COALESCE(SUM(bytes),0) FROM artifacts WHERE owner=?1",
            [&p.id],
            |r| r.get(0),
        )?;
        ensure!(
            used + content.len() as u64 <= self.artifact_quota_bytes,
            "artifact quota exhausted"
        );
        let id = Uuid::new_v4().to_string();
        let hash = digest(content.as_bytes());
        tx.execute(
            "INSERT INTO artifacts VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, p.id, content, hash, at + ttl, content.len() as u64],
        )?;
        tx.commit()?;
        Ok(
            json!({"id":id,"sha256":hash,"bytes":content.len(),"expires_at":at+ttl,"durability":"temporary_single_coordinator"}),
        )
    }
    pub fn artifact_get(&self, p: &Principal, id: &str, at: i64) -> Result<Value> {
        let row:Option<(String,String,i64)>=self.db.query_row("SELECT content,digest,expires_at FROM artifacts WHERE id=?1 AND owner=?2 AND expires_at>?3",
            params![id,p.id,at],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (content, hash, expires) = row.ok_or_else(|| anyhow::anyhow!("artifact not found"))?;
        Ok(json!({"id":id,"content":content,"sha256":hash,"expires_at":expires}))
    }
}

fn account(tx: &Transaction<'_>, id: &str) -> Result<()> {
    tx.execute("INSERT OR IGNORE INTO accounts(id) VALUES(?1)", [id])?;
    Ok(())
}
fn ledger(
    tx: &Transaction<'_>,
    reference: &str,
    owner: &str,
    kind: &str,
    amount: u64,
    job: Option<&str>,
    at: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO ledger VALUES(?1,?2,?3,?4,?5,?6)",
        params![reference, owner, kind, amount, job, at],
    )?;
    Ok(())
}
fn get(db: &Connection, id: &str) -> Result<Job> {
    let data: Option<String> = db
        .query_row("SELECT data FROM jobs WHERE id=?1", [id], |r| r.get(0))
        .optional()?;
    Ok(serde_json::from_str(
        &data.ok_or_else(|| anyhow::anyhow!("job not found"))?,
    )?)
}
pub(crate) fn save(tx: &Transaction<'_>, job: &Job) -> Result<()> {
    tx.execute(
        "UPDATE jobs SET status=?1,data=?2 WHERE id=?3",
        params![job.status, serde_json::to_string(job)?, job.id],
    )?;
    Ok(())
}
fn all_active(tx: &Transaction<'_>) -> Result<Vec<Job>> {
    let mut stmt =
        tx.prepare("SELECT data FROM jobs WHERE status IN ('queued','running','awaiting_payment') ORDER BY rowid")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}
fn check_lease(job: &Job, p: &Principal, lease: &str, at: i64) -> Result<()> {
    ensure!(
        p.has("worker")
            && job.status == "running"
            && job.worker.as_ref() == Some(&p.id)
            && job.lease_id.as_deref() == Some(lease)
            && job.lease_until.is_some_and(|t| t > at)
            && job.deadline > at
            && job.run_started_at.is_some_and(|t| t + MAX_RUN_SECONDS > at),
        "invalid or expired lease"
    );
    Ok(())
}
fn refund(tx: &Transaction<'_>, job: &mut Job, at: i64) -> Result<()> {
    if job.payment.is_some() {
        job.status = "refund_pending".into();
    }
    if job.reserved_credits > 0 {
        let amount = job.reserved_credits;
        tx.execute(
            "UPDATE accounts SET reserved=reserved-?1,balance=balance+?1 WHERE id=?2",
            params![amount, job.owner],
        )?;
        ledger(
            tx,
            &format!("refund:{}", job.id),
            &job.owner,
            "refund",
            amount,
            Some(&job.id),
            at,
        )?;
        job.reserved_credits = 0;
    }
    Ok(())
}
fn retry_or_fail(tx: &Transaction<'_>, job: &mut Job, at: i64, error: &str) -> Result<()> {
    if job.attempts >= MAX_ATTEMPTS || job.deadline <= at {
        job.status = "failed".into();
        refund(tx, job, at)?;
    } else {
        job.status = "queued".into();
    }
    job.error = Some(error.into());
    job.lease_id = None;
    job.lease_until = None;
    job.worker = None;
    job.run_started_at = None;
    Ok(())
}
