# Runtime 0.3.0-rc.9

This Linux x64 / Ubuntu WSL2 public test runtime includes the isolation-health
fix from Soapbox PR #2. It is a runtime-only release: it does not replace the
Bitterbot application or its wallet. Compute stays enabled; USDC remains limited
to Base Sepolia and the separate Compute wallet.

The host executable and the runner inside the Docker image must both be upgraded.
Use the included `containers/Dockerfile` with the included `bin/soapbox`; pin the
resulting immutable image ID in `worker.json`. Keep the existing model cache,
GPU lock, coordinator database, permissions, and pause settings. Back up the
database with SQLite's backup API before the first coordinator start: rc9 includes
the authorization/cancellation schema migrations introduced after rc5. A rollback
to rc5 requires its database backup and must not discard subsequently accepted work.

Stage and inspect the matching image before updating the main app's configured
operator binary. The release contains an allowlisted payload and SHA-256 manifest,
not credentials, model data, wallet files, databases, or the old Desktop overlay.
The Desktop setup wizard's baked-in runtime pin must be updated in a future
Desktop release for new enrollments; upgrading an already-configured operator uses
its explicit runtime binary and image configuration.

## Persistent Docker startup for WSL without systemd

The optional host installer must be explicitly invoked as root. It installs a
service command and points the WSL boot hook at it. It backs up existing settings,
preserves non-Docker WSL settings, refuses unrelated boot commands, and does not
restart Docker or WSL. It does not apply to distributions using systemd.

```sh
sudo python3 scripts/install-wsl-docker-service.py
# If replacing an existing Docker-only boot command, review it first, then use:
sudo python3 scripts/install-wsl-docker-service.py --replace-docker-boot-command
sudo /usr/local/sbin/soapbox-docker-service status
```

For a planned Docker restart, first pause workers and ensure jobs and builds are
idle, then run `sudo /usr/local/sbin/soapbox-docker-service restart`. Docker containers
may stop during this operation. The service uses a full system PATH and launches
the daemon in a child cgroup where swap accounting is visible. It does not disable
host swap or relax job limits. An already-running unhealthy daemon is reported,
not killed automatically. Use this service for manual restarts as well as WSL boot;
launching `dockerd` directly can recreate the original failure.

The boot hook takes effect on the next distribution start. A WSL shutdown interrupts
all processes in affected distributions; arrange that separately rather than having
the installer initiate it. The same boot command can be tested after a controlled
Docker stop without restarting the user's other WSL processes.

See [operator health](OPERATOR-HEALTH.md) for the pause/recovery semantics and
[validation](OPERATOR-HEALTH-VALIDATION.md) for the original worker tests.
