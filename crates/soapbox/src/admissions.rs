//! Gateway-managed peer admissions. Raw bearer credentials are never persisted here.
use crate::{config::Principal, model::digest, store::Store};
use anyhow::{ensure, Result};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub public_key: String,
    pub token: String,
    pub pools: Vec<String>,
    pub projects: Vec<String>,
}
impl Store {
    pub fn admit(&mut self, a: Admission) -> Result<Principal> {
        ensure!(
            a.public_key.len() == 64
                && a.public_key
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid public key"
        );
        ensure!(
            a.token.len() == 64 && a.token.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid credential"
        );
        ensure!(
            a.pools.len() + a.projects.len() > 0 && a.pools.len() + a.projects.len() <= 16,
            "invalid scopes"
        );
        ensure!(
            a.pools.iter().chain(&a.projects).all(|s| !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')),
            "invalid scope"
        );
        let principal = Principal {
            id: format!("mesh_{}", a.public_key),
            token_env: String::new(),
            roles: vec!["submitter".into()],
            pools: a.pools,
            projects: a.projects,
        };
        let count: u64 = self
            .db
            .query_row("SELECT COUNT(*) FROM mesh_admissions", [], |r| r.get(0))?;
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM mesh_admissions WHERE id=?1)",
            [&principal.id],
            |r| r.get(0),
        )?;
        ensure!(exists || count < 512, "admission limit reached");
        self.db.execute("INSERT INTO mesh_admissions(id,token_hash,principal) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET token_hash=excluded.token_hash,principal=excluded.principal", params![principal.id,digest(a.token.as_bytes()),serde_json::to_string(&principal)?])?;
        Ok(principal)
    }
    pub fn mesh_identity(&self, hash: &str) -> Result<Option<Principal>> {
        let data: Option<String> = self
            .db
            .query_row(
                "SELECT principal FROM mesh_admissions WHERE token_hash=?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }
    pub fn revoke_admission(&mut self, public_key: &str) -> Result<()> {
        let id = format!("mesh_{public_key}");
        // Stop accepting immediately; cancel outstanding work owned by this peer.
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM mesh_admissions WHERE id=?1", [&id])?;
        tx.commit()?;
        let p = Principal {
            id,
            token_env: String::new(),
            roles: vec!["submitter".into()],
            pools: vec![],
            projects: vec![],
        };
        let mut stmt = self
            .db
            .prepare("SELECT id FROM jobs WHERE owner=?1 AND status IN ('queued','running')")?;
        let jobs = stmt
            .query_map([&p.id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for job in jobs {
            self.cancel(&p, &job, crate::model::now())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_survives_restart_rotates_and_revokes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.db");
        let key = "ab".repeat(32);
        let token = "cd".repeat(32);
        let mut s = Store::open(&file, 1000, 1048576).unwrap();
        let p = s
            .admit(Admission {
                public_key: key.clone(),
                token: token.clone(),
                pools: vec!["team".into()],
                projects: vec![],
            })
            .unwrap();
        assert!(!p.has("admin"));
        assert!(!p.has("worker"));
        assert!(!p.permits(&crate::model::Mode::Paid {
            max_cost_credits: 1
        }));
        drop(s);
        let mut s = Store::open(&file, 1000, 1048576).unwrap();
        assert_eq!(
            s.mesh_identity(&digest(token.as_bytes()))
                .unwrap()
                .unwrap()
                .id,
            p.id
        );
        s.admit(Admission {
            public_key: key.clone(),
            token: "ef".repeat(32),
            pools: vec!["team".into()],
            projects: vec![],
        })
        .unwrap();
        assert!(s
            .mesh_identity(&digest(token.as_bytes()))
            .unwrap()
            .is_none());
        s.revoke_admission(&key).unwrap();
        assert!(s
            .mesh_identity(&digest("ef".repeat(32).as_bytes()))
            .unwrap()
            .is_none());
    }
}
