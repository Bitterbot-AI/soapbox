# Legacy Soapbox mapping

Reference source inspected in D:\Soapbox. It includes extracted/decompiled Java classes,
source archives and packaged distributions. This project does not modify or execute them.

| Java source / concept | Retained idea | Rust implementation |
|---|---|---|
| mNode/.../TranscodeNodesInfoStorageImpl.java | Active, idle and benchmark-qualified worker selection | Offer eligibility, heartbeats and exclusive leases; qualification benchmarks are Phase 2 |
| mNode/.../TranscodingVideoProcess.java | Split/assign work, confirmations, timeouts, reassignment | Generic queued/running/terminal state machine and retry cap |
| miner-1.0.4-SNAPSHOT.jar.src.zip / TranscodeMinerControllerImpl | Ready/fetch/execute/check/reward worker lifecycle | Independent worker polling, executing and completing leases |
| basenode/.../RewardCalculatorImpl.java | Explicit reward calculation | Fixed test-credit quote and atomic accounting |
| JNI transcoding backend | Backend separation | Workload adapter boundary; approved CPU function and Ollama |
| Regional discovery | Placement eligibility | Explicit pool/project policy now; verified region/topology in later phases |

We intentionally do not carry over the legacy video-specific proof system, byte tariffs,
coin emission schedule, bundled JRE, chain consensus or unaudited legacy dependencies.
No assumption is made that the packaged JNI implementation provides a reusable generic GPU runner.

The Rust code is a fresh implementation. It preserves useful control-flow concepts rather
than transliterating the Java application. The new resource and settlement protocols must be
reviewed against current workload requirements.
