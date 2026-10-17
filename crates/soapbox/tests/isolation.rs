//! Opt-in tests against the actual operator-owned container runtime.
use soapbox::{sandbox::arguments, worker::WorkerConfig};
use std::{process::Stdio, time::Duration};
use tokio::process::Command;

#[tokio::test]
#[ignore = "requires SOAPBOX_SANDBOX_CONFIG and Linux Docker"]
async fn enforced_filesystem_network_secrets_memory_and_cleanup() {
    let cfg = WorkerConfig::read(std::path::Path::new(
        &std::env::var("SOAPBOX_SANDBOX_CONFIG").unwrap(),
    ))
    .unwrap();
    let sandbox = cfg.sandbox.as_ref().unwrap();
    let cases=[
        (256, "test $(id -u) = 65532 && test ! -w /etc && test ! -e /var/run/docker.sock && test ! -e /mnt/d && test -z \"$SOAPBOX_CANARY_SECRET\" && test $(cat /sys/fs/cgroup/memory.swap.max) = 0 && test $(cat /sys/fs/cgroup/pids.max) = 128 && test $(ls /sys/class/net | wc -l) = 1 && test ! -w /models && grep -q 'NoNewPrivs:.*1' /proc/self/status && grep -q 'Seccomp:.*2' /proc/self/status", true),
        (64, "awk 'BEGIN { while (1) { x[i++]=i \"01234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789\" } }'", false),
    ];
    for (memory, script, success) in cases {
        let name = format!("soapbox-isolation-test-{}", uuid::Uuid::new_v4());
        let mut args = arguments(&name, sandbox, 1, memory, false);
        let entry = args
            .iter()
            .position(|v| v.starts_with("--entrypoint="))
            .unwrap();
        args[entry] = "--entrypoint=/bin/sh".into();
        args.pop();
        args.extend(["-c".into(), script.into()]);
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("docker")
                .args(args)
                .env("SOAPBOX_CANARY_SECRET", "must-not-enter-container")
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .output(),
        )
        .await;
        let _ = Command::new("docker")
            .args(["rm", "-f", &name])
            .output()
            .await;
        let output = result
            .expect("container must finish within timeout")
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !success {
            assert_eq!(
                output.status.code(),
                Some(137),
                "memory exhaustion must be killed by cgroup"
            );
        }
        let inspect = Command::new("docker")
            .args(["inspect", &name])
            .output()
            .await
            .unwrap();
        assert!(!inspect.status.success());
    }
}

#[tokio::test]
#[ignore = "requires SOAPBOX_SANDBOX_CONFIG and Linux Docker"]
async fn cpu_workload_runs_inside_the_same_boundary() {
    use soapbox::{config::Principal, model::*, store::Store};
    let cfg = WorkerConfig::read(std::path::Path::new(
        &std::env::var("SOAPBOX_SANDBOX_CONFIG").unwrap(),
    ))
    .unwrap();
    let buyer = Principal {
        id: "buyer".into(),
        token_env: "UNUSED".into(),
        roles: vec!["submitter".into()],
        pools: vec!["circle".into()],
        projects: vec![],
    };
    let mut store = Store::open(std::path::Path::new(":memory:"), 4, 1024).unwrap();
    let job = store
        .submit(
            &buyer,
            Submit {
                idempotency_key: "isolated-cpu".into(),
                mode: Mode::Private {
                    pool_id: "circle".into(),
                },
                workload: Workload::Sha256 {
                    texts: vec!["hello".into()],
                },
                resources: Resources::default(),
            },
            now(),
        )
        .unwrap();
    let result = soapbox::sandbox::execute(Some(&job), &cfg).await.unwrap();
    assert_eq!(
        result["hashes"][0],
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}
