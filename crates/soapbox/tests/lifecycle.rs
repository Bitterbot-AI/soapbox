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
        isolation: None,
        runtime: None,
        hardware: vec![],
        gpu_unavailable_reason: None,
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
        model_digest: None,
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

#[test]
fn pinned_gpu_results_require_matching_runtime_evidence_before_settlement() {
    let mut s = store();
    let buyer = principal("buyer");
    let worker = principal("worker");
    s.credit("buyer", 10, "fund", 100).unwrap();
    let mut spec = paid("gpu");
    spec.resources.require_gpu = true;
    spec.workload = Workload::Embeddings {
        model: "test:latest".into(),
        model_digest: Some("a".repeat(64)),
        texts: vec!["one".into()],
    };
    let submitted = s.submit(&buyer, spec.clone(), 100).unwrap();
    let mut gpu_offer = offer();
    gpu_offer.gpu_observed = true;
    gpu_offer.models = vec!["test:latest".into()];
    // A legacy boolean GPU claim is insufficient for pinned work.
    s.register(&worker, gpu_offer.clone(), 100).unwrap();
    assert!(s.claim(&worker, 100).unwrap().is_none());
    gpu_offer.runtime = Some(soapbox::gpu::Runtime {
        model: "test:latest".into(),
        model_digest: "a".repeat(64),
        version: "test".into(),
        model_bytes: 100,
        gpu_bytes: 100,
        observed_at: 100,
    });
    s.register(&worker, gpu_offer, 100).unwrap();
    let job = s.claim(&worker, 100).unwrap().unwrap();
    let completion = Completion {
        lease_id: job.lease_id.clone().unwrap(),
        result: json!({"embeddings":[[0.1,0.2]]}),
    };
    assert!(s.complete(&worker, &job.id, completion, 101).is_err());
    assert_eq!(s.balance("buyer").unwrap()["reserved_credits"], 1);
    let execution = json!({"backend":"ollama","model":"test:latest","model_digest":"a".repeat(64),
        "runtime_version":"test","fully_gpu_resident":true,"model_bytes":100,"gpu_bytes":100,"dimensions":2,
        "batches":1,"wall_ms":10,"total_duration_ns":2000000,"load_duration_ns":1000,"prompt_tokens":1,
        "hardware":[{"name":"RTX","uuid":"GPU-test","driver":"test","memory_total_mib":16000,
            "memory_free_mib":14000,"utilization_pct":0,"temperature_c":50}]});
    let mut result = json!({"embeddings":[[0.1,0.2]],"execution":execution});
    result["execution"]["model_digest"] = json!("b".repeat(64));
    assert!(s
        .complete(
            &worker,
            &job.id,
            Completion {
                lease_id: job.lease_id.clone().unwrap(),
                result: result.clone()
            },
            101
        )
        .is_err());
    result["execution"]["model_digest"] = json!("a".repeat(64));
    result["execution"]["fully_gpu_resident"] = json!(false);
    assert!(s
        .complete(
            &worker,
            &job.id,
            Completion {
                lease_id: job.lease_id.clone().unwrap(),
                result: result.clone()
            },
            101
        )
        .is_err());
    result["execution"]["fully_gpu_resident"] = json!(true);
    let completion = Completion {
        lease_id: job.lease_id.unwrap(),
        result,
    };
    let completed = s
        .complete(&worker, &job.id, completion.clone(), 101)
        .unwrap();
    s.complete(&worker, &job.id, completion, 102).unwrap();
    assert_eq!(
        completed.receipt.unwrap()["execution"]["model_digest"],
        "a".repeat(64)
    );
    assert_eq!(s.balance("worker").unwrap()["available_credits"], 1);
    assert_eq!(s.job(&submitted.id).unwrap().status, "succeeded");
    if let Workload::Embeddings { model_digest, .. } = &mut spec.workload {
        *model_digest = None;
    }
    spec.idempotency_key = "unpinned".into();
    assert!(s.submit(&buyer, spec, 103).is_err());
}

fn usdc_config() -> soapbox::payment::PaymentConfig {
    soapbox::payment::PaymentConfig {
        rpc_url: "http://127.0.0.1:8545".into(),
        chain_id: 31337,
        token: format!("0x{}", "1".repeat(40)),
        escrow: format!("0x{}", "2".repeat(40)),
        escrow_code_sha256: "a".repeat(64),
        microusdc_per_item: 1000,
        confirmations: 1,
        wallets: [
            ("buyer".into(), format!("0x{}", "3".repeat(40))),
            ("worker".into(), format!("0x{}", "4".repeat(40))),
        ]
        .into(),
    }
}
fn observation(state: u64, result: &str) -> soapbox::payment::Observation {
    soapbox::payment::Observation {
        state,
        result: result.into(),
        block: "0x10".into(),
        hash: format!("0x{}", "b".repeat(64)),
    }
}
#[test]
fn usdc_requires_funding_isolation_bound_operator_and_confirmed_payout() {
    let mut s = store();
    s.payments = Some(usdc_config());
    let at = now();
    let buyer = principal("buyer");
    let worker = principal("worker");
    let request = spec(
        "usdc",
        Mode::PaidUsdc {
            max_cost_microusdc: 2000,
            operator: "worker".into(),
        },
    );
    let quoted = s.submit(&buyer, request.clone(), at).unwrap();
    assert_eq!(quoted.status, "awaiting_payment");
    assert_eq!(s.submit(&buyer, request, at).unwrap().id, quoted.id);
    s.register(&worker, offer(), at).unwrap();
    assert!(s.claim(&worker, at).unwrap().is_none());
    s.apply_payment(&quoted.id, observation(1, "")).unwrap();
    assert!(
        s.claim(&worker, at).unwrap().is_none(),
        "shared-host worker cannot earn USDC"
    );
    let mut isolated = offer();
    isolated.isolation = Some("docker_v1".into());
    s.register(&principal("other"), isolated.clone(), at)
        .unwrap();
    assert!(
        s.claim(&principal("other"), at).unwrap().is_none(),
        "quote pins operator"
    );
    s.register(&worker, isolated, at).unwrap();
    let leased = s.claim(&worker, at).unwrap().unwrap();
    let completed = finish(&mut s, &worker, &leased, at + 1);
    assert_eq!(completed.status, "settlement_pending");
    assert_eq!(s.balance("worker").unwrap()["available_credits"], 0);
    assert!(s
        .apply_payment(&quoted.id, observation(2, "0xwrong"))
        .is_err());
    let hash = format!(
        "0x{}",
        completed.receipt.unwrap()["result_sha256"]
            .as_str()
            .unwrap()
    );
    let paid = s.apply_payment(&quoted.id, observation(2, &hash)).unwrap();
    assert_eq!(paid.status, "succeeded");
    assert_eq!(paid.receipt.unwrap()["payment_state"], "paid");
    assert_eq!(
        s.apply_payment(&quoted.id, observation(2, &hash))
            .unwrap()
            .status,
        "succeeded"
    );
    assert!(s.apply_payment(&quoted.id, observation(3, "")).is_err());
}
#[test]
fn usdc_cancellation_cannot_requeue_after_late_funding_and_can_void_unfunded_quote() {
    let mut s = store();
    s.payments = Some(usdc_config());
    let at = now();
    let buyer = principal("buyer");
    let job = s
        .submit(
            &buyer,
            spec(
                "cancel-usdc",
                Mode::PaidUsdc {
                    max_cost_microusdc: 2000,
                    operator: "worker".into(),
                },
            ),
            at,
        )
        .unwrap();
    assert_eq!(
        s.cancel(&buyer, &job.id, at).unwrap().status,
        "refund_pending"
    );
    assert_eq!(
        s.apply_payment(&job.id, observation(1, "")).unwrap().status,
        "refund_pending"
    );
    assert_eq!(
        s.apply_payment(&job.id, observation(3, "")).unwrap().status,
        "refunded"
    );
    let job = s
        .submit(
            &buyer,
            spec(
                "void-usdc",
                Mode::PaidUsdc {
                    max_cost_microusdc: 2000,
                    operator: "worker".into(),
                },
            ),
            at,
        )
        .unwrap();
    s.cancel(&buyer, &job.id, at).unwrap();
    assert_eq!(
        s.apply_payment(&job.id, observation(4, "")).unwrap().status,
        "cancelled"
    );
}
#[test]
fn payment_config_rejects_mainnet_wrong_circle_token_and_over_budget() {
    let mut cfg = usdc_config();
    cfg.chain_id = 8453;
    assert!(cfg.validate().is_err());
    cfg.chain_id = 84532;
    cfg.confirmations = 2;
    assert!(cfg.validate().is_err());
    let mut s = store();
    s.payments = Some(usdc_config());
    assert!(s
        .submit(
            &principal("buyer"),
            spec(
                "low",
                Mode::PaidUsdc {
                    max_cost_microusdc: 1,
                    operator: "worker".into()
                }
            ),
            now()
        )
        .is_err());
}
