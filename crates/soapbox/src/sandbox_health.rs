//! Short public health reasons; bounded Docker diagnostics stay in local logs.
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    future::Future,
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt};

const MARKER: &str = "SOAPBOX_SANDBOX_ERROR ";
pub(crate) const DIAGNOSTIC_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Code {
    #[serde(rename = "sandbox_swap_unenforced")]
    SwapUnenforced,
    #[serde(rename = "sandbox_boundary_invalid")]
    BoundaryInvalid,
    #[serde(rename = "sandbox_probe_failed")]
    ProbeFailed,
    #[serde(rename = "sandbox_unavailable")]
    Unavailable,
    #[serde(rename = "sandbox_timeout")]
    Timeout,
    #[serde(rename = "sandbox_output_limit")]
    OutputLimit,
    #[serde(rename = "sandbox_runtime_failed")]
    RuntimeFailed,
}
impl Code {
    fn message(self) -> &'static str {
        match self {
            Self::SwapUnenforced => {
                "Swap isolation failed; check Docker cgroups. CPU/GPU work paused."
            }
            Self::BoundaryInvalid => {
                "Sandbox isolation failed; check Docker cgroups and security settings."
            }
            Self::ProbeFailed => {
                "Isolation check failed; check Docker and rebuild the matching sandbox image."
            }
            Self::Unavailable => "Docker sandbox unavailable; check Docker and the pinned image.",
            Self::Timeout => "Sandbox timed out; check Docker and local resource pressure.",
            Self::OutputLimit => {
                "Sandbox output exceeded its limit; inspect the local operator log."
            }
            Self::RuntimeFailed => "Isolated runtime failed; inspect the local operator log.",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Failure {
    pub code: Code,
    pub diagnostic: String,
}
impl Failure {
    pub fn new(code: Code, diagnostic: impl AsRef<str>) -> Self {
        let text = diagnostic.as_ref();
        let mut start = text.len().saturating_sub(DIAGNOSTIC_LIMIT);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        Self {
            code,
            diagnostic: text[start..]
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .collect(),
        }
    }
    pub fn blocks_execution(&self) -> bool {
        self.code != Code::RuntimeFailed
    }
    pub fn emit(&self) {
        eprintln!("Sandbox diagnostic (local only): {}", self.diagnostic);
        // Only fixed, known reason codes cross the subprocess/public-status boundary.
        eprintln!(
            "{MARKER}{}",
            serde_json::to_string(&self.code).expect("reason code")
        );
    }
    pub fn from_stderr(stderr: &[u8]) -> Self {
        let text = String::from_utf8_lossy(stderr);
        let code = text
            .lines()
            .rev()
            .find_map(|line| {
                line.strip_prefix(MARKER)
                    .and_then(|s| serde_json::from_str::<Code>(s).ok())
            })
            .unwrap_or(Code::RuntimeFailed);
        Self::new(code, &text)
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = serde_json::to_value(self.code).map_err(|_| fmt::Error)?;
        write!(
            f,
            "{}: {}",
            code.as_str().ok_or(fmt::Error)?,
            self.code.message()
        )
    }
}
impl std::error::Error for Failure {}

/// Drain stderr to avoid pipe deadlocks, retaining a bounded tail so leading
/// Docker warnings cannot hide the sandbox's final structured failure.
pub(crate) async fn diagnostic_tail(reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    diagnostic_tail_until(reader, std::future::pending()).await
}

/// Stop once the owning process has been reaped, even if a descendant inherited
/// its stderr pipe. No detached reader task survives cancellation.
pub(crate) async fn diagnostic_tail_until(
    mut reader: impl AsyncRead + Unpin,
    stop: impl Future<Output = ()>,
) -> std::io::Result<Vec<u8>> {
    tokio::pin!(stop);
    let mut tail = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = tokio::select! {
            biased;
            _ = &mut stop => return Ok(tail),
            result = reader.read(&mut chunk) => result?,
        };
        if n == 0 {
            return Ok(tail);
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > DIAGNOSTIC_LIMIT {
            tail.drain(..tail.len() - DIAGNOSTIC_LIMIT);
        }
    }
}

/// Re-probe healthy sandboxes every thirty seconds; retry failures after
/// 2, 4, 8, 16, then 30 seconds. This never owns the user's pause state.
#[derive(Default)]
pub(crate) struct Health {
    next: Option<Instant>,
    failures: u32,
    error: Option<Failure>,
}
impl Health {
    pub async fn check<F, Fut>(&mut self, now: Instant, probe: F) -> Option<Failure>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), Failure>>,
    {
        if self.next.is_some_and(|next| now < next) {
            return self.error.clone();
        }
        let started = Instant::now();
        let result = probe().await;
        self.record(now + started.elapsed(), result);
        self.error.clone()
    }
    pub fn invalidate(&mut self, now: Instant, error: Failure) {
        self.record(now, Err(error));
    }
    fn record(&mut self, now: Instant, result: Result<(), Failure>) {
        let next_code = result.as_ref().err().map(|e| e.code);
        if next_code != self.error.as_ref().map(|e| e.code) {
            if let Err(error) = &result {
                eprintln!(
                    "Compute execution paused: {error}\nSandbox diagnostic (local only): {}",
                    error.diagnostic
                );
            } else {
                eprintln!(
                    "Compute sandbox isolation recovered; operator pause settings still apply"
                );
            }
        }
        let delay = if result.is_err() {
            self.failures = self.failures.saturating_add(1);
            (1u64 << self.failures.min(5)).min(30)
        } else {
            self.failures = 0;
            30
        };
        self.error = result.err();
        self.next = Some(now + Duration::from_secs(delay));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn completed_runtime_does_not_wait_for_inherited_stderr_eof() {
        let (reader, _still_open_descendant) = tokio::io::duplex(64);
        let tail = tokio::time::timeout(
            Duration::from_secs(1),
            diagnostic_tail_until(reader, async {}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(tail.is_empty());
    }
    #[tokio::test]
    async fn warning_flood_cannot_hide_sandbox_failure_or_leak_into_public_reason() {
        let stderr = format!(
            "{}\n{MARKER}\"sandbox_swap_unenforced\"\nError: boundary refused",
            "WARNING: local-path-sensitive-detail\n".repeat(1000)
        );
        let tail = diagnostic_tail(stderr.as_bytes()).await.unwrap();
        assert_eq!(tail.len(), DIAGNOSTIC_LIMIT);
        let error = Failure::from_stderr(&tail);
        assert_eq!(error.code, Code::SwapUnenforced);
        assert!(error.to_string().len() <= 120);
        assert!(!error.to_string().contains("sensitive"));
        assert!(error.diagnostic.contains("WARNING"));
    }
    #[test]
    fn unknown_markers_are_not_trusted_and_every_summary_fits_old_clients() {
        let error = Failure::from_stderr(b"WARNING\nSOAPBOX_SANDBOX_ERROR \"secret\"\n");
        assert_eq!(error.code, Code::RuntimeFailed);
        for code in [
            Code::SwapUnenforced,
            Code::BoundaryInvalid,
            Code::ProbeFailed,
            Code::Unavailable,
            Code::Timeout,
            Code::OutputLimit,
            Code::RuntimeFailed,
        ] {
            assert!(Failure::new(code, "secret").to_string().len() <= 120);
        }
    }
    #[tokio::test]
    async fn health_rechecks_after_success_and_backs_off_until_recovery() {
        let mut health = Health::default();
        let start = Instant::now();
        assert!(health.check(start, || async { Ok(()) }).await.is_none());
        assert!(health
            .check(start + Duration::from_secs(29), || async {
                panic!("cached")
            })
            .await
            .is_none());
        let mut at = health.next.unwrap();
        for delay in [2, 4, 8, 16, 30, 30] {
            let error = health
                .check(at, || async {
                    Err(Failure::new(Code::SwapUnenforced, "diagnostic"))
                })
                .await;
            assert!(error.is_some());
            let next = health.next.unwrap();
            assert!(next >= at + Duration::from_secs(delay));
            assert!(health
                .check(next - Duration::from_secs(1), || async {
                    panic!("backoff")
                })
                .await
                .is_some());
            at = next;
        }
        assert!(health.check(at, || async { Ok(()) }).await.is_none());
        assert_eq!(health.failures, 0);
    }
}
