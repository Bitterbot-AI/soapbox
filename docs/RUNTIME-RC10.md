# Runtime 0.3.0-rc.10

This public Linux x64 / Ubuntu WSL2 test runtime fixes isolated Ollama startup
on a cold or busy host. Startup uses an actual 60-second wall-clock deadline,
validates the version response, and preserves a bounded local stderr tail on
failure. Child processes and log readers are cleaned up on every exit path.

Cold GPU readiness has a separate budget: 180 seconds inside the sandbox,
190 seconds for Docker, and 200 seconds in the worker. Actual jobs retain
their 100-second sandbox and 110-second Docker limits. A readiness check is
not an operator pause. Failed checks never advertise GPU capacity as ready.

The archive contains the host executable and Dockerfile. Build the image from
that same executable and pin its immutable image ID. Preserve existing models,
GPU lock, database, peer permissions, and manual pause settings. Do not upgrade
the host executable alone. The WSL Docker service from rc9 remains available;
it does not change host configuration automatically.

Compute stays enabled by default in Bitterbot. Paid work remains Base Sepolia
test USDC (chain 84532), using the separate Compute wallet. This release does
not enable mainnet settlement or paid P2P batches.

The underlying readiness fix passed 54 Rust tests and Clippy, two isolated
RTX 4080 readiness probes, and a real first-attempt paid embedding job with
an exact 0.002000 test-USDC operator payout. This evidence used separate
buyer/operator profiles on one physical machine; cross-machine payment
validation remains outstanding. Release packaging adds a version bump, so
archive installation and the matching image are checked separately.

Rollback requires restoring the previous host executable and image together.
No database schema changes are introduced relative to rc9. Keep both runtimes
until the upgraded operator has passed its readiness probe.
