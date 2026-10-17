# Runbook and Bitterbot integration

> Current isolation and USDC extension: [Phase 2 isolation and payments](PHASE-2-ISOLATION-PAYMENTS.md). Earlier TEST_CREDIT/shared-host descriptions below refer to the original pilot.

## Development on this machine

The Java source is D:\Soapbox. The new repository is D:\Bitterbot\soapbox.
The available Rust toolchain is in WSL Ubuntu. No native Windows Rust toolchain was installed.

From WSL:

```sh
cd /mnt/d/Bitterbot/soapbox
export PATH=/home/vicmg/.nvm/versions/node/v22.22.1/bin:$PATH
CARGO_TARGET_DIR=/tmp/soapbox-target cargo build --locked
SOAPBOX_BINARY=/tmp/soapbox-target/debug/soapbox node scripts/demo.mjs
```

For a fresh machine use ordinary cargo build --locked, then node scripts/demo.mjs.
The Node script needs Node 22+ and no npm packages.

## Independent service

Copy examples/coordinator.json to config.local.json and provision your own principal IDs,
roles, pools and projects. Set each referenced environment variable to a distinct random
32-byte hex secret. The example requires SOAPBOX_ADMIN_TOKEN, SOAPBOX_CLIENT_TOKEN,
SOAPBOX_WORKER_TOKEN and SOAPBOX_WORKER2_TOKEN.

```sh
cargo run -- serve --config config.local.json --db data/soapbox.sqlite
cargo run -- worker --config examples/worker.json
```

Use a second terminal for the worker. Each concurrent worker requires its own principal
and token. For worker-2 use --token-env SOAPBOX_WORKER2_TOKEN.
The worker makes outbound requests, so inbound access to each worker is unnecessary.

For remote workers, terminate TLS at the coordinator and restrict the plain HTTP listener.
Clients refuse non-loopback HTTP. This is a trusted pilot: use private networking and do not
publish the service directly to arbitrary buyers.

Use request for scripted calls:

```sh
cargo run -- request --method POST --path /v1/jobs --file examples/private-job.json
cargo run -- request --path /v1/status
```

To issue test credits, an admin POSTs to /v1/credits with account, amount and idempotency_key.
This is a test balance, never USDC or money. The demo demonstrates funding automatically.

## Contribution settings

examples/worker.json explicitly lists allowed modes, pool IDs, collective projects, minimum
accepted paid quote and RAM admission allowance. One process offers one execution slot.
Change configuration and restart the worker to alter its local allowlist.

Creating the configured pause file prevents new leases; deleting it resumes offers.
An operator may also pause a worker through the authenticated API. Server-side pause survives
worker heartbeat registration. Current jobs drain; cancellation is a separate operation.

This version does not infer user idleness, battery state or thermals. Running a worker is
an explicit opt-in to contribute within the documented pilot workload limits.

## GPU embeddings

See [PHASE-2-GPU.md](PHASE-2-GPU.md) for the complete setup and execution contract.
Set both ollama_model and ollama_digest in worker configuration. The operator installs
the model; the worker can warm that pinned model automatically. Full GPU residency,
NVIDIA telemetry and admission policy are required before advertising a ready GPU offer.
GPU job requests include the same model_digest. Version 0.2 rejects boolean-only GPU
claims and missing execution evidence. CPU-only v0.1 jobs remain compatible.

## Bitterbot connection

Configure the gateway process environment, never the renderer:

| Variable | Purpose |
|---|---|
| BITTERBOT_SOAPBOX_ENABLED=1 | Explicitly enable the adapter |
| BITTERBOT_SOAPBOX_URL | Coordinator root URL; default http://127.0.0.1:8787 |
| BITTERBOT_SOAPBOX_TOKEN | Provisioned submitter credential; keep server-side |
| BITTERBOT_SOAPBOX_BINARY | Optional absolute path to a native executable for gateway process ownership |
| BITTERBOT_SOAPBOX_CONFIG | Optional coordinator config; setting it starts a managed coordinator |
| BITTERBOT_SOAPBOX_DB | Required database path for a managed coordinator |
| BITTERBOT_SOAPBOX_WORKER_CONFIG | Optional separate opt-in to launch a worker |
| SOAPBOX_WORKER_TOKEN | Credential for the managed worker |
| Other token variables named in coordinator config | Required only when the gateway launches that coordinator |

A managed coordinator requires an HTTP 127.0.0.1 URL; external services can use HTTPS.
Configure file paths as absolute paths. The gateway runs the binary without a shell.
For WSL development run the external demo and connect by URL; a Windows process cannot
directly spawn the Linux binary. Production Windows packaging requires a native build.

With demo --keep running, load .local/demo.env into the gateway's environment:

```sh
set -a
. /mnt/d/Bitterbot/soapbox/.local/demo.env
set +a
# Start your existing Bitterbot gateway command from this shell.
```

For a Windows gateway in PowerShell:

```powershell
Get-Content D:\Bitterbot\soapbox\.local\demo.env | ForEach-Object {
  $entry = $_ -split '=', 2
  [Environment]::SetEnvironmentVariable($entry[0], $entry[1], 'Process')
}
# Start your existing Bitterbot gateway command from this shell.
```

WSL localhost forwarding must be available for a Windows gateway to reach the demo.
Do not commit or share demo.env. The demo uses a buyer credential, so worker administration
remains unavailable unless you deliberately provision an admin integration credential.

Open **Advanced → Compute**. The view shows connectivity, balances, offers and jobs;
submits CPU or configured embedding work; cancels pending work; retrieves receipts;
and stores/retrieves temporary artifacts. Refresh recovers from a connection error.

Gateway RPC authorization:
- operator.read: soapbox.status, jobs, job, artifact.get
- operator.write: submit, cancel, artifact.put
- operator.admin: worker.pause

The gateway credential represents one Soapbox account. This is an operator-console integration,
not per-chat-user delegation. Do not expose a shared funded gateway account to untrusted users.

## Recovery and operations

SQLite WAL/FULL transactions preserve committed jobs and reservations. After restart, leases
remain valid until expiry; maintenance requeues them or refunds final failures.
Workers reconnect with bounded backoff. Stop all processes cleanly before copying the database,
or use SQLite's online backup API. Include WAL state if taking filesystem snapshots.

Monitor active queue depth, stale workers, failed/retried jobs and reserved balances.
The active queue is capped at 256 jobs. History and receipts need archival/disk management
for longer deployment. There is no HA failover in this release.

Do not edit balances or job JSON manually. Use authenticated requests and keep a database backup.
For credential revocation, update coordinator configuration/environment and restart it.


Native Windows validation: an unsigned development executable is now available at .local/bin/soapbox.exe. Run scripts/demo.ps1, or scripts/demo.ps1 -Keep for UI use. The complete coordinator/two-worker demo passed on Windows. See VALIDATION.md.
