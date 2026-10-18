# Legacy Soapbox mapping

Reference material: D:\Soapbox, including decompiled Java, source archives and packaged
distributions. The original was broader than a job scheduler: it combined a private
Ethereum-derived chain, geo identities, deposits, Kademlia discovery, storage/cache and
partitioned transcoding. The Rust implementation is a fresh execution service, not a
port of that entire network or economy.

| Original subsystem | Current implementation | Deliberate gap |
|---|---|---|
| Ethereum-derived chain, deposits and emission | Explicit lease/accounting state; Base Sepolia USDC escrow | No private consensus chain or new token |
| Kademlia / network identities | Bitterbot's existing libp2p identity, gossip and encrypted peer RPC, integrated in desktop | Soapbox standalone does not run a second DHT |
| Geo identities and locality | Explicit pool/project membership | Verified regional placement and latency benchmarks remain |
| Partitioned transcoding | Generic durable jobs; desktop batch partitioning and ordered CPU/GPU result aggregation | No video workload in the current scope |
| Worker polling, confirmations and reward | Heartbeats, exclusive leases, retries, owner isolation, metering and escrow | Paid multi-operator fan-out remains a separate phase |
| Storage/cache, byte tariffs | Owner-scoped temporary artifacts | No durable replicated storage, cache economy or availability proof |
| Qualification and proof | CPU output recomputation; pinned GPU execution and shape checks | GPU mathematical verification / replicated checking remain |
| Admission and identity | Runtime peer-bound submitter admissions with hashed tokens, scope limits and revocation | Operator grants remain local; circle membership does not confer execution rights |

The desktop mesh uses signed expiring capacity descriptors and peer-approved private or
collective batches. It does not gossip inputs or bearer credentials. Existing paid jobs
remain on Base Sepolia with a dedicated Compute wallet; mesh admissions cannot submit paid
jobs. Current capacity advertisements are self-reported, not certified benchmarks.

The legacy video proof system, JNI backend, JRE, byte tariffs, coin schedule and consensus
dependencies are not reused. Generic GPU execution cannot be inferred from the old JNI
transcoding package. These omissions are product boundaries, not completed ports.
