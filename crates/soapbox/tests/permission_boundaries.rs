use soapbox::{admissions::Admission, config::Principal, model::*, store::Store};

fn admit(s: &mut Store, pools: &[&str], projects: &[&str], token: &str) -> Principal {
    s.admit(Admission {
        public_key: "ab".repeat(32),
        token: token.into(),
        pools: pools.iter().map(|p| (*p).into()).collect(),
        projects: projects.iter().map(|p| (*p).into()).collect(),
    })
    .unwrap()
}
fn spec(key: &str, mode: Mode) -> Submit {
    Submit {
        idempotency_key: key.into(),
        mode,
        workload: Workload::Sha256 {
            texts: vec!["synthetic".into()],
        },
        resources: Resources::default(),
    }
}
fn private(key: &str, pool: &str) -> Submit {
    spec(
        key,
        Mode::Private {
            pool_id: pool.into(),
        },
    )
}
fn worker(s: &mut Store) -> Principal {
    let p = Principal {
        id: "worker".into(),
        token_env: String::new(),
        roles: vec!["worker".into()],
        pools: vec!["team".into(), "other".into()],
        projects: vec!["science".into()],
    };
    let offer: Offer = serde_json::from_value(serde_json::json!({
        "cpu_threads":1,"memory_mib":256,"gpu_observed":false,"models":[],
        "modes":["private","collective"],"pools":["team","other"],"projects":["science"]
    }))
    .unwrap();
    s.register(&p, offer, now()).unwrap();
    p
}
fn completion(job: &Job) -> Completion {
    Completion {
        lease_id: job.lease_id.clone().unwrap(),
        result: job.spec.workload.cpu_result().unwrap(),
    }
}
fn store() -> Store {
    Store::open(std::path::Path::new(":memory:"), 100, 1048576).unwrap()
}

#[test]
fn current_scope_cancels_queued_and_running_but_preserves_retained_scope() {
    let mut s = store();
    let token = "cd".repeat(32);
    let p = admit(&mut s, &["team", "other"], &["science"], &token);
    let running = s.submit(&p, private("running", "team"), now()).unwrap();
    let w = worker(&mut s);
    let lease = s.claim(&w, now()).unwrap().unwrap();
    assert_eq!(lease.id, running.id);
    let queued = s
        .submit(
            &p,
            spec(
                "queued",
                Mode::Collective {
                    project_id: "science".into(),
                },
            ),
            now(),
        )
        .unwrap();
    let retained = s.submit(&p, private("retained", "other"), now()).unwrap();
    admit(&mut s, &["other"], &[], &token);
    assert_eq!(s.job(&running.id).unwrap().status, "cancelled");
    assert_eq!(s.job(&queued.id).unwrap().status, "cancelled");
    assert!(s
        .renew(&w, &lease.id, lease.lease_id.as_ref().unwrap(), now())
        .is_err());
    assert!(s
        .complete(&w, &lease.id, completion(&lease), now())
        .is_err());
    assert!(s.submit(&p, private("stale-scope", "team"), now()).is_err());
    assert_eq!(s.claim(&w, now()).unwrap().unwrap().id, retained.id);
}

#[test]
fn captured_credential_cannot_enqueue_after_revoke_or_rotation_or_readmission() {
    let mut s = store();
    let old = "cd".repeat(32);
    let new = "ef".repeat(32);
    let p = admit(&mut s, &["team"], &[], &old);
    s.revoke_admission(&"ab".repeat(32)).unwrap();
    assert!(s
        .submit_authenticated(
            &p,
            &digest(old.as_bytes()),
            private("revoked", "team"),
            now()
        )
        .is_err());
    admit(&mut s, &["team"], &[], &new);
    assert!(s
        .submit_authenticated(
            &p,
            &digest(old.as_bytes()),
            private("readmitted", "team"),
            now()
        )
        .is_err());
    assert!(s
        .submit_authenticated(
            &p,
            &digest(new.as_bytes()),
            private("current", "team"),
            now()
        )
        .is_ok());
    admit(&mut s, &["other"], &[], &new);
    assert!(s
        .submit_authenticated(
            &p,
            &digest(new.as_bytes()),
            private("narrowed", "team"),
            now()
        )
        .is_err());
}

#[test]
fn revoke_and_scope_update_roll_back_entirely_if_cancellation_fails() {
    let mut s = store();
    let token = "cd".repeat(32);
    let p = admit(&mut s, &["team"], &[], &token);
    let job = s.submit(&p, private("atomic", "team"), now()).unwrap();
    s.db.execute_batch("CREATE TRIGGER reject_cancel BEFORE UPDATE ON jobs WHEN NEW.status='cancelled' BEGIN SELECT RAISE(ABORT,'injected cancellation fault'); END;").unwrap();
    assert!(s.revoke_admission(&"ab".repeat(32)).is_err());
    assert!(s
        .mesh_identity(&digest(token.as_bytes()))
        .unwrap()
        .is_some());
    assert_eq!(s.job(&job.id).unwrap().status, "queued");
    assert!(s
        .admit(Admission {
            public_key: "ab".repeat(32),
            token: "ef".repeat(32),
            pools: vec!["other".into()],
            projects: vec![]
        })
        .is_err());
    let current = s.mesh_identity(&digest(token.as_bytes())).unwrap().unwrap();
    assert_eq!(current.pools, vec!["team"]);
    assert!(s
        .mesh_identity(&digest("ef".repeat(32).as_bytes()))
        .unwrap()
        .is_none());
    s.db.execute_batch("DROP TRIGGER reject_cancel").unwrap();
    s.revoke_admission(&"ab".repeat(32)).unwrap();
    assert_eq!(s.job(&job.id).unwrap().status, "cancelled");
}

#[test]
fn legacy_orphans_cannot_renew_complete_or_dispatch_and_are_swept_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.sqlite");
    let mut s = Store::open(&file, 100, 1048576).unwrap();
    let p = admit(&mut s, &["team"], &[], &"cd".repeat(32));
    s.submit(&p, private("running", "team"), now()).unwrap();
    let w = worker(&mut s);
    let lease = s.claim(&w, now()).unwrap().unwrap();
    let queued = s.submit(&p, private("queued", "team"), now()).unwrap();
    // Emulate the durable state left by the old delete-then-cancel implementation.
    s.db.execute("DELETE FROM mesh_admissions", []).unwrap();
    drop(s);
    let mut s = Store::open(&file, 100, 1048576).unwrap();
    assert!(s
        .renew(&w, &lease.id, lease.lease_id.as_ref().unwrap(), now())
        .is_err());
    assert!(s
        .complete(&w, &lease.id, completion(&lease), now())
        .is_err());
    assert!(s
        .fail(&w, &lease.id, lease.lease_id.as_ref().unwrap(), now())
        .is_err());
    assert!(s.claim(&w, now()).unwrap().is_none());
    assert_eq!(s.job(&queued.id).unwrap().status, "cancelled");
    assert_eq!(s.job(&lease.id).unwrap().status, "cancelled");
}
