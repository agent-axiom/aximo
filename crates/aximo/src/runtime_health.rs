use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;

#[derive(Clone)]
pub struct RuntimeHealth {
    inner: Arc<Mutex<RuntimeHealthState>>,
    degrade_after_consecutive_failures: u64,
    recovery_cooldown: Duration,
}

#[derive(Default)]
struct RuntimeHealthState {
    components: BTreeMap<String, ComponentState>,
}

#[derive(Default)]
struct ComponentState {
    consecutive_failures: u64,
    reason: Option<String>,
    last_failure_at: Option<Instant>,
    recovery_probe_in_flight: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentAdmission {
    Allowed,
    RecoveryProbe,
    Rejected,
}

impl RuntimeHealth {
    pub fn new(degrade_after_consecutive_failures: u64) -> Self {
        Self::with_recovery_cooldown(degrade_after_consecutive_failures, Duration::from_secs(30))
    }

    pub fn with_recovery_cooldown(
        degrade_after_consecutive_failures: u64,
        recovery_cooldown: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeHealthState::default())),
            degrade_after_consecutive_failures,
            recovery_cooldown,
        }
    }

    pub fn record_success(&self, component: impl Into<String>) {
        let mut state = self.inner.lock().expect("runtime health lock");
        let component = state.components.entry(component.into()).or_default();
        component.consecutive_failures = 0;
        component.reason = None;
        component.last_failure_at = None;
        component.recovery_probe_in_flight = false;
    }

    pub fn record_failure(&self, component: impl Into<String>, reason: impl Into<String>) {
        let mut state = self.inner.lock().expect("runtime health lock");
        let component = state.components.entry(component.into()).or_default();
        component.consecutive_failures = component.consecutive_failures.saturating_add(1);
        component.reason = Some(reason.into());
        component.last_failure_at = Some(Instant::now());
        component.recovery_probe_in_flight = false;
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

    pub fn is_component_ready(&self, component: &str) -> bool {
        let state = self.inner.lock().expect("runtime health lock");
        state
            .components
            .get(component)
            .map(|state| self.component_readiness(component, state).status == "ready")
            .unwrap_or(true)
    }

    pub fn admit_component(&self, component: impl Into<String>) -> ComponentAdmission {
        let mut state = self.inner.lock().expect("runtime health lock");
        let component = state.components.entry(component.into()).or_default();
        if !self.is_degraded(component) {
            return ComponentAdmission::Allowed;
        }

        if self.recovery_probe_available(component) {
            component.recovery_probe_in_flight = true;
            return ComponentAdmission::RecoveryProbe;
        }

        ComponentAdmission::Rejected
    }

    pub fn can_admit_component(&self, component: &str) -> bool {
        let state = self.inner.lock().expect("runtime health lock");
        state.components.get(component).is_none_or(|component| {
            !self.is_degraded(component) || self.recovery_probe_available(component)
        })
    }

    pub fn cancel_recovery_probe(&self, component: &str) {
        let mut state = self.inner.lock().expect("runtime health lock");
        if let Some(component) = state.components.get_mut(component) {
            component.recovery_probe_in_flight = false;
        }
    }

    fn component_readiness(&self, component: &str, state: &ComponentState) -> ComponentReadiness {
        let degraded = self.is_degraded(state);

        ComponentReadiness {
            component: component.to_string(),
            status: if degraded { "degraded" } else { "ready" },
            consecutive_failures: state.consecutive_failures,
            reason: if degraded { state.reason.clone() } else { None },
        }
    }

    fn is_degraded(&self, state: &ComponentState) -> bool {
        self.degrade_after_consecutive_failures > 0
            && state.consecutive_failures >= self.degrade_after_consecutive_failures
    }

    fn recovery_probe_available(&self, state: &ComponentState) -> bool {
        if state.recovery_probe_in_flight {
            return false;
        }

        state
            .last_failure_at
            .is_none_or(|last_failure_at| last_failure_at.elapsed() >= self.recovery_cooldown)
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

    #[test]
    fn degraded_component_allows_single_recovery_probe_after_cooldown() {
        let health = RuntimeHealth::with_recovery_cooldown(1, Duration::from_millis(5));

        health.record_failure("short:parakeet", "runtime failure");
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::Rejected
        );

        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::RecoveryProbe
        );
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::Rejected
        );

        health.record_success("short:parakeet");
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::Allowed
        );
    }

    #[test]
    fn failed_recovery_probe_reopens_component_until_next_cooldown() {
        let health = RuntimeHealth::with_recovery_cooldown(1, Duration::from_millis(5));

        health.record_failure("short:parakeet", "runtime failure");
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::RecoveryProbe
        );

        health.record_failure("short:parakeet", "probe failed");
        assert_eq!(
            health.admit_component("short:parakeet"),
            ComponentAdmission::Rejected
        );
    }
}
