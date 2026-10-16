use serde_json::json;
use soapbox::{config::Principal, model::*, store::Store};
fn principal(id: &str) -> Principal {
    Principal {
        id: id.into(),
        token_env: "UNUSED".into(),
        roles: vec!["submitter".into(), "worker".into()],
        pools: vec!["circle".into()],
        projects: vec!["science".into()],
    }
}
fn store() -> Store {
    Store::open(std::path::Path::new(":memory:"), 4, 1024).unwrap()
}
fn spec(key: &str, mode: Mode) -> Submit {
    Submit {
        idempotency_key: key.into(),
        mode,
        workload: Workload::Sha256 {
            texts: vec!["hello".into(), "world".into()],
        },
        resources: Resources::default(),
    }
}
fn offer() -> Offer {
    Offer {
        cpu_threads: 1,
        memory_mib: 512,
        gpu_observed: false,
        models: vec![],
        modes: vec!["paid".into(), "private".into(), "collective".into()],
        pools: vec!["circle".into()],
        projects: vec!["science".into()],
        paused: false,
        min_price_credits: 1,
    }
}
fn paid(key: &str) -> Submit {
    spec(
        key,
        Mode::Paid {
            max_cost_credits: 2,
        },
    )
}
fn private(key: &str) -> Submit {
    spec(
        key,
        Mode::Private {
            pool_id: "circle".into(),
        },
    )
}
fn finish(s: &mut Store, p: &Principal, job: &Job, at: i64) -> Job {
    s.complete(
        p,
        &job.id,
        Completion {
            lease_id: job.lease_id.clone().unwrap(),
            result: job.spec.workload.cpu_result().unwrap(),
        },
        at,
    )
    .unwrap()
}

#[test]
fn paid_reservation_and_completion_are_idempotent() {
    let mut s = store();
    let buyer = principal("buyer");
    let worker = principal("worker");
    s.credit("buyer", 10, "fund", 100).unwrap();
    s.credit("buyer", 10, "fund", 100).unwrap();
    assert!(s.credit("buyer", 11, "fund", 100).is_err());
    let first = s.submit(&buyer, paid("one"), 100).unwrap();
    assert_eq!(first.id, s.submit(&buyer, paid("one"), 100).unwrap().id);
    assert_eq!(s.balance("buyer").unwrap()["available_credits"], 8);
    assert_eq!(s.balance("buyer").unwrap()["reserved_credits"], 2);
    s.register(&worker, offer(), 100).unwrap();
    let leased = s.claim(&worker, 100).unwrap().unwrap();
    assert_eq!(finish(&mut s, &worker, &leased, 101).status, "succeeded");
    finish(&mut s, &worker, &leased, 102);
    assert_eq!(s.balance("worker").unwrap()["available_credits"], 2);
    assert_eq!(s.balance("buyer").unwrap()["reserved_credits"], 0);
    assert_eq!(s.balance("buyer").unwrap()["available_credits"], 8);
}
#[test]
fn cancellation_refunds_and_rejects_late_result() {
    let mut s = store();
    let buyer = principal("buyer");
    let w = principal("worker");
    s.credit("buyer", 10, "fund", 100).unwrap();
    let submitted = s.submit(&buyer, paid("a"), 100).unwrap();
    s.register(&w, offer(), 100).unwrap();
    let lease = s.claim(&w, 100).unwrap().unwrap();
    s.cancel(&buyer, &submitted.id, 101).unwrap();
    s.cancel(&buyer, &submitted.id, 102).unwrap();
    assert_eq!(s.balance("buyer").unwrap()["available_credits"], 10);
    assert!(s
        .complete(
            &w,
            &lease.id,
            Completion {
                lease_id: lease.lease_id.unwrap(),
                result: lease.spec.workload.cpu_result().unwrap()
            },
            103
        )
        .is_err());
}
#[test]
fn private_pool_enforced_at_submission_and_dispatch() {
    let mut s = store();
    let buyer = principal("buyer");
    let mut outsider = principal("outsider");
    outsider.pools.clear();
    assert!(s.submit(&outsider, private("no"), 100).is_err());
    s.submit(&buyer, private("yes"), 100).unwrap();
    let registered = s.register(&outsider, offer(), 100).unwrap();
    assert!(registered.offer.pools.is_empty());
    assert!(s.claim(&outsider, 100).unwrap().is_none());
}
#[test]
fn collective_quotas_are_per_project_and_day_and_retries_do_not_double_count() {
    let mut s = store();
    let p = principal("p");
    let job = spec(
        "one",
        Mode::Collective {
            project_id: "science".into(),
        },
    );
    s.submit(&p, job.clone(), 100).unwrap();
    s.submit(&p, job, 100).unwrap();
    s.submit(
        &p,
        spec(
            "two",
            Mode::Collective {
                project_id: "science".into(),
            },
        ),
        100,
    )
    .unwrap();
    assert!(s
        .submit(
            &p,
            spec(
                "three",
                Mode::Collective {
                    project_id: "science".into()
                }
            ),
            100
        )
        .is_err());
    s.submit(
        &p,
        spec(
            "tomorrow",
            Mode::Collective {
                project_id: "science".into(),
            },
        ),
        86401,
    )
    .unwrap();
    assert_eq!(s.balance("p").unwrap()["available_credits"], 0);
}
#[test]
fn worker_can_hold_only_one_lease_and_pause_survives_heartbeats() {
    let mut s = store();
    let p = principal("p");
    s.submit(&p, private("one"), 100).unwrap();
    s.submit(&p, private("two"), 100).unwrap();
    s.register(&p, offer(), 100).unwrap();
    let first = s.claim(&p, 100).unwrap().unwrap();
    assert!(s.claim(&p, 100).unwrap().is_none());
    finish(&mut s, &p, &first, 101);
    s.pause("p", true).unwrap();
    s.register(&p, offer(), 102).unwrap();
    assert!(s.claim(&p, 102).unwrap().is_none());
    s.pause("p", false).unwrap();
    assert!(s.claim(&p, 102).unwrap().is_some());
}
#[test]
fn expired_lease_is_reassigned_and_old_attempt_cannot_settle() {
    let mut s = store();
    let buyer = principal("buyer");
    let w1 = principal("w1");
    let w2 = principal("w2");
    s.submit(&buyer, private("one"), 100).unwrap();
    s.register(&w1, offer(), 100).unwrap();
    let old = s.claim(&w1, 100).unwrap().unwrap();
    s.register(&w2, offer(), 131).unwrap();
    let new = s.claim(&w2, 131).unwrap().unwrap();
    assert_eq!(old.id, new.id);
    assert_eq!(new.attempts, 2);
    assert_ne!(old.lease_id, new.lease_id);
    assert!(s
        .complete(
            &w1,
            &old.id,
            Completion {
                lease_id: old.lease_id.unwrap(),
                result: old.spec.workload.cpu_result().unwrap()
            },
            132
        )
        .is_err());
    finish(&mut s, &w2, &new, 132);
}
#[test]
fn retry_exhaustion_refunds_paid_budget() {
    let mut s = store();
    let p = principal("p");
    s.credit("p", 10, "fund", 100).unwrap();
    let job = s.submit(&p, paid("a"), 100).unwrap();
    for at in [100, 131, 162] {
        s.register(&p, offer(), at).unwrap();
        s.claim(&p, at).unwrap().unwrap();
        s.sweep(at + 31).unwrap();
    }
    assert_eq!(s.job(&job.id).unwrap().status, "failed");
    assert_eq!(s.balance("p").unwrap()["available_credits"], 10);
}
#[test]
fn queue_deadline_releases_reservation_without_a_worker() {
    let mut s = store();
    let p = principal("p");
    s.credit("p", 10, "fund", 100).unwrap();
    let job = s.submit(&p, paid("a"), 100).unwrap();
    s.sweep(701).unwrap();
    assert_eq!(s.job(&job.id).unwrap().status, "failed");
    assert_eq!(s.balance("p").unwrap()["reserved_credits"], 0);
}
#[test]
fn heartbeat_cannot_extend_execution_forever() {
    let mut s = store();
    let p = principal("p");
    s.submit(&p, private("a"), 100).unwrap();
    s.register(&p, offer(), 100).unwrap();
    let j = s.claim(&p, 100).unwrap().unwrap();
    let lease = j.lease_id.unwrap();
    for at in [120, 140, 160, 180, 200, 219] {
        s.renew(&p, &j.id, &lease, at).unwrap();
    }
    assert!(s.renew(&p, &j.id, &lease, 220).is_err());
}
#[test]
fn invalid_result_cannot_trigger_payment() {
    let mut s = store();
    let p = principal("p");
    s.credit("p", 10, "fund", 100).unwrap();
    s.submit(&p, paid("a"), 100).unwrap();
    s.register(&p, offer(), 100).unwrap();
    let j = s.claim(&p, 100).unwrap().unwrap();
    assert!(s
        .complete(
            &p,
            &j.id,
            Completion {
                lease_id: j.lease_id.unwrap(),
                result: json!({"hashes":["forged"]})
            },
            101
        )
        .is_err());
    assert_eq!(s.balance("p").unwrap()["reserved_credits"], 2);
}
#[test]
fn resource_constraints_and_stale_workers_prevent_dispatch() {
    let mut s = store();
    let p = principal("p");
    let mut j = private("a");
    j.resources.memory_mib = 1024;
    s.submit(&p, j, 100).unwrap();
    s.register(&p, offer(), 100).unwrap();
    assert!(s.claim(&p, 100).unwrap().is_none());
    let mut bigger = offer();
    bigger.memory_mib = 2048;
    s.register(&p, bigger, 100).unwrap();
    assert!(s.claim(&p, 131).unwrap().is_none());
}
#[test]
fn artifacts_enforce_owner_quota_and_expiry() {
    let mut s = store();
    let p = principal("p");
    let other = principal("other");
    let item = s.artifact_put(&p, "data", 60, 100).unwrap();
    let id = item["id"].as_str().unwrap();
    assert!(s.artifact_get(&other, id, 100).is_err());
    assert_eq!(s.artifact_get(&p, id, 101).unwrap()["content"], "data");
    assert!(s.artifact_put(&p, &"x".repeat(1024), 60, 101).is_err());
    assert!(s.artifact_get(&p, id, 160).is_err());
    s.sweep(160).unwrap();
    s.artifact_put(&p, &"x".repeat(1024), 60, 161).unwrap();
}
#[test]
fn idempotency_conflicts_and_unfunded_jobs_are_rejected() {
    let mut s = store();
    let p = principal("p");
    s.submit(&p, private("a"), 100).unwrap();
    assert!(s.submit(&p, paid("a"), 100).is_err());
    assert!(s.submit(&p, paid("b"), 100).is_err());
    assert_eq!(s.jobs(&p).unwrap().len(), 1);
}
#[test]
fn embedding_validation_reports_limited_assurance() {
    let w = Workload::Embeddings {
        model: "test".into(),
        texts: vec!["hello".into()],
    };
    assert_eq!(
        w.verify(&json!({"embeddings":[[0.1,0.2]]})).unwrap(),
        "shape_only_trusted_worker"
    );
    assert!(w.verify(&json!({"embeddings":[[0.1],[0.2]]})).is_err());
    assert!(w.verify(&json!({"embeddings":[[]]})).is_err());
}
#[test]
fn reservations_and_jobs_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let p = principal("p");
    let id = {
        let mut s = Store::open(&path, 100, 1024).unwrap();
        s.credit("p", 10, "fund", 100).unwrap();
        s.submit(&p, paid("a"), 100).unwrap().id
    };
    let mut s = Store::open(&path, 100, 1024).unwrap();
    assert_eq!(s.balance("p").unwrap()["reserved_credits"], 2);
    s.cancel(&p, &id, 101).unwrap();
    assert_eq!(s.balance("p").unwrap()["available_credits"], 10);
}
