# Operator isolation health and recovery

This describes the Rust worker change after `v0.3.0-rc.8`. Previously published
rc8 archives do not contain it. A matching host executable **and sandbox image**
must ship together before Desktop can use this behavior.

Compute remains enabled by default. A broken execution boundary pauses this
operator's CPU and GPU capacity; it does not disable Compute or change a wallet.
Payments remain on Base Sepolia through the separate Compute wallet.

## Admission and recovery

Before advertising an isolated worker, Soapbox launches a CPU-only container
using the same confinement flags and boundary checks as a job. The hidden
`sandbox-check` command verifies the boundary and exits without loading a model,
reserving a GPU, or contacting the coordinator. This also checks CPU-only workers.

The probe has a thirty-second deadline to accommodate cold container startup.
After a successful probe, the next probe becomes due thirty seconds later.
Failures retry after 2, 4, 8, 16, then 30 seconds, measured from probe completion.
These are minimum delays: polling, Docker startup, an active job, or coordinator
availability can extend them. There is no separate background probe during a job.
Every job still checks its own container boundary before executing; a cached
health result never bypasses that check.

On failed isolation, the worker registers a paused offer and does not claim work.
On a boundary failure during execution, it withdraws capacity, reports the failed
lease through the existing job path, and schedules another health check. Existing
job retry/refund/settlement rules apply; a health failure does not prove a refund.

After a successful retry, the worker can advertise capacity again, subject to RAM,
GPU/model admission, its pause file, and the coordinator's operator pause. Health
recovery never removes the pause file or clears `operator_paused`. A GPU-only
problem can still leave CPU available when isolation is healthy. An isolation
failure must not be presented as a CPU-only fallback.

Legacy `allow_unsafe_host` development workers are outside this Docker check and
remain ineligible for USDC execution.

## Desktop and wire compatibility

No offer fields were added. Existing coordinators retain the same schema:

```json
{
  "paused": true,
  "isolation": null,
  "gpu_observed": false,
  "models": [],
  "runtime": null,
  "gpu_unavailable_reason": "sandbox_swap_unenforced: Swap isolation failed; check Docker cgroups. CPU/GPU work paused."
}
```

Despite its legacy field name, `gpu_unavailable_reason` can describe loss of the
whole execution boundary. Desktop should use the offer's pause/isolation state
as well as the reason when deciding whether CPU work is available.

Public reasons use a stable code plus fixed text, each at most 120 bytes:

| Code | Meaning |
| --- | --- |
| `sandbox_swap_unenforced` | The container's `memory.swap.max` is not zero. |
| `sandbox_boundary_invalid` | Another required boundary check failed. |
| `sandbox_probe_failed` | Probe response/command failed, including incompatible images. |
| `sandbox_unavailable` | Docker could not be spawned. Check Docker and the image. |
| `sandbox_timeout` | The bounded sandbox operation timed out. |
| `sandbox_output_limit` | Sandbox stdout exceeded its limit. |
| `sandbox_runtime_failed` | Runtime failed without a recognized boundary marker. |

An unsuccessful health probe always pauses execution, including unclassified
runtime errors (converted to `sandbox_probe_failed`). A standalone GPU runtime
failure after a healthy isolation check need not pause CPU capacity.

The runner emits a known reason marker after its diagnostic. The host drains
stderr while retaining only its last 8 KiB, so leading Docker warnings do not
hide the final boundary reason or block the pipe. Bounded diagnostics remain in
the local operator log; they are not sent in offers. Support bundles should still
be reviewed for local paths and other private information before sharing.

## Troubleshooting Docker and WSL

Use the same Docker context and user as the operator service:

```sh
docker info --format 'cgroup={{.CgroupVersion}} swap={{.SwapLimit}}'
soapbox inspect --config /absolute/path/to/worker.json
```

Expect cgroup v2 and swap support. These daemon flags are a preliminary check;
the in-container check is authoritative. Docker can be installed and GPU devices
visible while the required resource limits are unavailable. See Docker's
[resource constraint documentation](https://docs.docker.com/engine/containers/resource_constraints/).

On WSL, prefer a daemon managed by the distribution's service manager or the
supported Docker Desktop integration. Neither is a substitute for verification.
A manually launched daemon, especially after reboot or outside its expected
service cgroup, needs investigation if the checks fail.

If the operator chooses to enable systemd in a compatible WSL distribution,
merge this setting into `/etc/wsl.conf`, preserving existing settings:

```ini
[boot]
systemd=true
```

Follow Microsoft's [WSL systemd instructions](https://learn.microsoft.com/en-us/windows/wsl/systemd).
Applying this normally requires shutting down WSL from Windows, which interrupts
other WSL services. Arrange that explicitly with the operator, then verify the
Docker service and rerun `inspect`. Do not have the wizard rewrite host settings,
restart WSL, launch competing daemons, disable swap, or relax sandbox flags to
make a check pass. The worker resumes automatically once the actual boundary is
healthy, unless an independent pause remains set.

## Packaging and Desktop handoff

1. Publish a new versioned runtime archive containing this host executable and
   rebuild the sandbox image with the same runner. Do not replace rc8 assets in
   place. Record archive checksums and the immutable image ID.
2. Update Desktop's runtime pin and upgrade path. Existing operator images must
   also be rebuilt/replaced; swapping only the host executable leaves the old
   runner unable to execute `sandbox-check` and intentionally pauses the worker.
3. Recheck prerequisites on operator service startup, not just initial setup.
   Surface the short reason before clipping text; keep diagnostics in local logs.
4. Show an execution pause for invalid isolation. Do not silently claim CPU is
   available. Retry with backoff and preserve explicit user/operator pauses.
5. Retest startup, post-setup regression, recovery, manual pauses, noisy Docker
   stderr, and image-version mismatch. Keep Compute enabled by default and the
   Base Sepolia wallet separate from mainnet throughout the upgrade.

## Verification

See the [local validation record](OPERATOR-HEALTH-VALIDATION.md) for the automated
and real-container checks, including the old-image compatibility failure.

`cargo test --workspace --locked` includes:

- Known reason selection after a Docker warning flood and bounded local output.
- Healthy-result caching, failure backoff, and automatic recovery.
- CPU and valid GPU jobs rejected after isolation fails, with positive healthy
  admission controls and preservation of the pause file.
- On Unix, a real worker process plus HTTP coordinator and SQLite: a child-only
  Docker shim injects failure, jobs stay queued with zero attempts, repair honors
  both local and coordinator pauses, work succeeds after explicit unpause, and
  a later regression withdraws capacity again.

The shim test does not require or claim a real GPU, Docker boundary, peer machine,
or payment. Real-container checks must separately demonstrate healthy, deliberately
nonzero container swap, and recovered states using a matching image. Never break
the host daemon or production worker to inject that failure.
