use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Clone)]
pub struct RuntimeHealth {
    inner: Arc<Mutex<RuntimeHealthState>>,
    degrade_after_consecutive_failures: u64,
}

#[derive(Default)]
struct RuntimeHealthState {
    consecutive_failures: u64,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub status: &'static str,
    pub consecutive_failures: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RuntimeHealth {
    pub fn new(degrade_after_consecutive_failures: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeHealthState::default())),
            degrade_after_consecutive_failures,
        }
    }

    pub fn record_success(&self) {
        let mut state = self.inner.lock().expect("runtime health lock");
        state.consecutive_failures = 0;
        state.reason = None;
    }

    pub fn record_failure(&self, reason: impl Into<String>) {
        let mut state = self.inner.lock().expect("runtime health lock");
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.reason = Some(reason.into());
    }

    pub fn readiness(&self) -> Readiness {
        let state = self.inner.lock().expect("runtime health lock");
        let degraded = self.degrade_after_consecutive_failures > 0
            && state.consecutive_failures >= self.degrade_after_consecutive_failures;

        Readiness {
            status: if degraded { "degraded" } else { "ready" },
            consecutive_failures: state.consecutive_failures,
            reason: if degraded { state.reason.clone() } else { None },
        }
    }

    pub fn is_ready(&self) -> bool {
        self.readiness().status == "ready"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_degrades_after_configured_failure_threshold() {
        let health = RuntimeHealth::new(2);

        assert!(health.is_ready());

        health.record_failure("first failure");
        assert!(health.is_ready());

        health.record_failure("second failure");
        let readiness = health.readiness();
        assert_eq!(readiness.status, "degraded");
        assert_eq!(readiness.consecutive_failures, 2);
        assert_eq!(readiness.reason.as_deref(), Some("second failure"));
    }

    #[test]
    fn success_clears_consecutive_failures() {
        let health = RuntimeHealth::new(1);

        health.record_failure("runtime failure");
        assert!(!health.is_ready());

        health.record_success();
        let readiness = health.readiness();
        assert_eq!(readiness.status, "ready");
        assert_eq!(readiness.consecutive_failures, 0);
        assert!(readiness.reason.is_none());
    }
}
