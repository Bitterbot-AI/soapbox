# Operator health validation — 2026-10-22

Scope: the post-rc8 Rust sandbox-health change, tested on one Windows host through
WSL Ubuntu. This is source validation, not evidence of a published or installed
Desktop release. See [behavior and packaging requirements](OPERATOR-HEALTH.md).

## Automated checks

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test --workspace --locked`: 48 passed; 2 opt-in external GPU tests ignored.
- The new Unix integration test ran the actual worker, HTTP coordinator, and
  SQLite store with a child-only Docker shim. Broken isolation left queued work
  at zero attempts. Recovery preserved the pause file and coordinator pause.
  After explicit unpause, the job succeeded once. A later failure withdrew the
  offer again without terminating the worker.

Unit checks cover bounded warning-heavy stderr, reason codes fitting 120 bytes,
backoff/recovery, and rejection of both valid CPU and GPU jobs after a healthy
offer loses isolation. They do not claim actual GPU execution.

## Real Docker checks

The daemon reported cgroup v2 and swap support. A disposable image replaced only
the runner in an existing local pinned sandbox base with the tested executable.
The host binary used its normal `inspect` command and launched actual containers.
The test configuration used 256 MiB RAM, no model, and no GPU assignment.

| Case | Result |
| --- | --- |
| Normal `--memory=256m --memory-swap=256m` | `paused=false`, `isolation=docker_v1` |
| Child-only wrapper changes swap allowance to `512m` | `paused=true`, `isolation=null`, `sandbox_swap_unenforced` |
| Restore the normal arguments | `paused=false`, `isolation=docker_v1` |
| New host with an old runner image lacking `sandbox-check` | `paused=true`, `isolation=null`, `sandbox_probe_failed`; underlying command error retained locally |

The nonzero-swap case deliberately changes only a disposable container's limits;
the host daemon, cgroups, WSL settings, and installed operator were not modified.
The public reason was:

```text
sandbox_swap_unenforced: Swap isolation failed; check Docker cgroups. CPU/GPU work paused.
```

An initial cold-start probe exceeded ten seconds. The implementation now gives
the probe thirty seconds, and the final healthy/failure/recovery sequence passed.
Healthy results are cached for thirty seconds; per-job boundary verification is
unchanged.

## Limits

These checks used one physical host, no GPU workload, no remote peer, and no
payment. They do not establish cross-node performance, main-app upgrade behavior,
or on-chain settlement. Compute defaults and wallet/payment code were unchanged.
Desktop still needs a new versioned runtime, matching image upgrade, prerequisite
messaging, and its own UI/upgrade tests before this change reaches users.
