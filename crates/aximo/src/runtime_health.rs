use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;

#[derive(Clone)]
pub struct RuntimeHealth {
    inner: Arc<Mutex<RuntimeHealthState>>,
    degrade_after_consecutive_failures: u64,
}

#[derive(Default)]
struct RuntimeHealthState {
    components: BTreeMap<String, ComponentState>,
}

#[derive(Default)]
struct ComponentState {
    consecutive_failures: u64,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub status: &'static str,
    pub consecutive_failures: u64,
    pub components: Vec<ComponentReadiness>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentReadiness {
    pub component: String,
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

    pub fn record_success(&self, component: impl Into<String>) {
        let mut state = self.inner.lock().expect("runtime health lock");
        let component = state.components.entry(component.into()).or_default();
        component.consecutive_failures = 0;
        component.reason = None;
    }

    pub fn record_failure(&self, component: impl Into<String>, reason: impl Into<String>) {
        let mut state = self.inner.lock().expect("runtime health lock");
        let component = state.components.entry(component.into()).or_default();
        component.consecutive_failures = component.consecutive_failures.saturating_add(1);
        component.reason = Some(reason.into());
    }

    pub fn readiness(&self) -> Readiness {
        let state = self.inner.lock().expect("runtime health lock");
        let components = state
            .components
            .iter()
            .map(|(component, state)| self.component_readiness(component, state))
            .collect::<Vec<_>>();
        let degraded = components
            .iter()
            .any(|component| component.status == "degraded");
        let consecutive_failures = components
            .iter()
            .map(|component| component.consecutive_failures)
            .max()
            .unwrap_or_default();

        Readiness {
            status: if degraded { "degraded" } else { "ready" },
            consecutive_failures,
            components,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.readiness().status == "ready"
    }

    fn component_readiness(&self, component: &str, state: &ComponentState) -> ComponentReadiness {
        let degraded = self.degrade_after_consecutive_failures > 0
            && state.consecutive_failures >= self.degrade_after_consecutive_failures;

        ComponentReadiness {
            component: component.to_string(),
            status: if degraded { "degraded" } else { "ready" },
            consecutive_failures: state.consecutive_failures,
            reason: if degraded { state.reason.clone() } else { None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_degrades_after_configured_failure_threshold() {
        let health = RuntimeHealth::new(2);

        assert!(health.is_ready());

        health.record_failure("short:parakeet", "first failure");
        assert!(health.is_ready());

        health.record_failure("short:parakeet", "second failure");
        let readiness = health.readiness();
        assert_eq!(readiness.status, "degraded");
        assert_eq!(readiness.consecutive_failures, 2);
        assert_eq!(readiness.components.len(), 1);
        assert_eq!(readiness.components[0].component, "short:parakeet");
        assert_eq!(readiness.components[0].status, "degraded");
        assert_eq!(
            readiness.components[0].reason.as_deref(),
            Some("second failure")
        );
    }

    #[test]
    fn success_clears_consecutive_failures() {
        let health = RuntimeHealth::new(1);

        health.record_failure("short:parakeet", "runtime failure");
        assert!(!health.is_ready());

        health.record_success("short:parakeet");
        let readiness = health.readiness();
        assert_eq!(readiness.status, "ready");
        assert_eq!(readiness.consecutive_failures, 0);
        assert_eq!(readiness.components[0].status, "ready");
        assert!(readiness.components[0].reason.is_none());
    }

    #[test]
    fn success_only_clears_matching_component() {
        let health = RuntimeHealth::new(1);

        health.record_failure("short:parakeet", "short failed");
        health.record_success("realtime_final:parakeet");

        let readiness = health.readiness();
        assert_eq!(readiness.status, "degraded");
        assert_eq!(readiness.components.len(), 2);
        assert_eq!(readiness.components[0].component, "realtime_final:parakeet");
        assert_eq!(readiness.components[0].status, "ready");
        assert_eq!(readiness.components[1].component, "short:parakeet");
        assert_eq!(readiness.components[1].status, "degraded");
    }
}
