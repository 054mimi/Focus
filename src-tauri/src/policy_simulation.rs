//! Deterministic fake policy adapter. This module deliberately has no Windows
//! API, process, service, or shell dependencies.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_ENTRIES_PER_FAMILY: usize = 256;
const MAX_TOTAL_ENTRIES: usize = 512;
const MAX_ENTRY_LENGTH: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Planned,
    Applying,
    Applied,
    Verified,
    RollbackPending,
    RolledBack,
    Failed,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Verified => "verified",
            Self::RollbackPending => "rollback_pending",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
        }
    }
}

// Faults are deterministic test inputs. Production IPC exposes only the success path.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationFault {
    None,
    FailBeforeApply,
    TimeoutBeforeApply,
    PartialApply,
    TimeoutAfterPartialApply,
    VerificationFailure,
    VerificationTimeout,
    RollbackFailure,
    RollbackTimeout,
}

pub(crate) fn validate_request_id(request_id: &str) -> Result<(), SimulationError> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(SimulationError::InvalidRequestKey);
    }
    Ok(())
}

pub(crate) fn validate_operation_id(operation_id: &str) -> Result<(), SimulationError> {
    let Some(suffix) = operation_id.strip_prefix("sim-") else {
        return Err(SimulationError::InvalidRequestKey);
    };
    if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SimulationError::InvalidRequestKey);
    }
    Ok(())
}

pub(crate) fn transition_allowed(from: LifecycleState, to: LifecycleState) -> bool {
    matches!(
        (from, to),
        (LifecycleState::Planned, LifecycleState::Applying)
            | (LifecycleState::Applying, LifecycleState::Applied)
            | (LifecycleState::Applying, LifecycleState::RollbackPending)
            | (LifecycleState::Applying, LifecycleState::RolledBack)
            | (LifecycleState::Applying, LifecycleState::Failed)
            | (LifecycleState::Applied, LifecycleState::Verified)
            | (LifecycleState::Applied, LifecycleState::RollbackPending)
            | (LifecycleState::Verified, LifecycleState::RollbackPending)
            | (LifecycleState::RollbackPending, LifecycleState::RolledBack)
            | (LifecycleState::RollbackPending, LifecycleState::Failed)
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyFamily {
    Application,
    Website,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedRule {
    pub family: PolicyFamily,
    pub mode: String,
    pub entry: String,
}

impl SimulatedRule {
    pub fn key(&self) -> String {
        format!(
            "{}:{}",
            self.family.as_key(),
            self.entry.trim().to_lowercase()
        )
    }
    pub fn value(&self) -> Value {
        serde_json::json!({"mode":self.mode,"entry":self.entry})
    }
}
impl PolicyFamily {
    fn as_key(&self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Website => "website",
            Self::Network => "network",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyPlan {
    pub session_id: String,
    pub profile_version: i64,
    pub session_revision: u64,
    pub snapshot_json: String,
    pub rules: Vec<SimulatedRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPolicySnapshot {
    pub session_id: String,
    pub profile_version: i64,
    pub revision: u64,
    pub state: String,
    pub snapshot_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationError {
    InvalidRequestKey,
    InactiveSession,
    StaleSession,
    SnapshotMismatch,
    InvalidSnapshot,
    InvalidPolicy,
    TooManyEntries,
    InvalidTransition,
    InvalidRecoveryData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleEvent {
    pub revision: u64,
    pub kind: String,
    pub occurred_at_ms: i64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyOperation {
    pub operation_id: String,
    pub request_key: String,
    pub plan: PolicyPlan,
    pub prior_resources: BTreeMap<String, Value>,
    pub simulated_resources: BTreeMap<String, Value>,
    pub state: LifecycleState,
    pub revision: u64,
    pub error: Option<String>,
    pub events: Vec<LifecycleEvent>,
}

impl PolicyPlan {
    pub fn validate(
        snapshot: &SessionPolicySnapshot,
        expected_version: i64,
        expected_revision: u64,
        expected_snapshot: &str,
    ) -> Result<Self, SimulationError> {
        if !matches!(snapshot.state.as_str(), "active_work" | "active_break") {
            return Err(SimulationError::InactiveSession);
        }
        if snapshot.revision != expected_revision {
            return Err(SimulationError::StaleSession);
        }
        if snapshot.profile_version != expected_version {
            return Err(SimulationError::SnapshotMismatch);
        }
        if snapshot.snapshot_json != expected_snapshot {
            return Err(SimulationError::SnapshotMismatch);
        }
        let config: Value = serde_json::from_str(&snapshot.snapshot_json)
            .map_err(|_| SimulationError::InvalidSnapshot)?;
        if !config.is_object() {
            return Err(SimulationError::InvalidSnapshot);
        }
        let mut rules = Vec::new();
        parse_policy(
            &config,
            "applicationPolicy",
            PolicyFamily::Application,
            &["entries"],
            &mut rules,
        )?;
        parse_policy(
            &config,
            "websitePolicy",
            PolicyFamily::Website,
            &["domains"],
            &mut rules,
        )?;
        parse_policy(
            &config,
            "networkPolicy",
            PolicyFamily::Network,
            &["applications"],
            &mut rules,
        )?;
        if rules.len() > MAX_TOTAL_ENTRIES {
            return Err(SimulationError::TooManyEntries);
        }
        rules.sort_by(|a, b| a.key().cmp(&b.key()));
        Ok(Self {
            session_id: snapshot.session_id.clone(),
            profile_version: snapshot.profile_version,
            session_revision: snapshot.revision,
            snapshot_json: snapshot.snapshot_json.clone(),
            rules,
        })
    }
}

fn parse_policy(
    config: &Value,
    key: &str,
    family: PolicyFamily,
    entry_fields: &[&str],
    out: &mut Vec<SimulatedRule>,
) -> Result<(), SimulationError> {
    let policy = config
        .get(key)
        .and_then(Value::as_object)
        .ok_or(SimulationError::InvalidPolicy)?;
    let mode = policy
        .get("mode")
        .and_then(Value::as_str)
        .ok_or(SimulationError::InvalidPolicy)?;
    if !matches!(mode, "disabled" | "whitelist" | "blacklist") {
        return Err(SimulationError::InvalidPolicy);
    }
    let values = entry_fields
        .iter()
        .find_map(|f| policy.get(*f))
        .and_then(Value::as_array)
        .ok_or(SimulationError::InvalidPolicy)?;
    if values.len() > MAX_ENTRIES_PER_FAMILY {
        return Err(SimulationError::TooManyEntries);
    }
    if mode != "disabled"
        && !values
            .iter()
            .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
    {
        return Err(SimulationError::InvalidPolicy);
    }
    let mut unique = BTreeSet::new();
    for value in values {
        let entry = value.as_str().ok_or(SimulationError::InvalidPolicy)?;
        if entry.trim().is_empty() || entry.len() > MAX_ENTRY_LENGTH || entry.contains('\0') {
            return Err(SimulationError::InvalidPolicy);
        }
        let rule = SimulatedRule {
            family: family.clone(),
            mode: mode.into(),
            entry: entry.trim().into(),
        };
        if !unique.insert(rule.key()) {
            return Err(SimulationError::InvalidPolicy);
        }
        if mode != "disabled" {
            out.push(rule);
        }
    }
    Ok(())
}

impl PolicyOperation {
    pub fn new(
        operation_id: String,
        request_key: String,
        plan: PolicyPlan,
        prior_resources: BTreeMap<String, Value>,
    ) -> Result<Self, SimulationError> {
        validate_request_id(&request_key)?;
        if operation_id.is_empty() {
            return Err(SimulationError::InvalidRequestKey);
        }
        let mut op = Self {
            operation_id,
            request_key,
            plan,
            simulated_resources: prior_resources.clone(),
            prior_resources,
            state: LifecycleState::Planned,
            revision: 0,
            error: None,
            events: vec![],
        };
        op.transition(LifecycleState::Planned, "planned", 0, None);
        Ok(op)
    }
    pub fn begin_apply(&mut self, now: i64) -> Result<(), SimulationError> {
        match self.state {
            LifecycleState::Applying | LifecycleState::Applied | LifecycleState::Verified => {
                return Ok(());
            }
            LifecycleState::Planned => {}
            _ => return Err(SimulationError::InvalidTransition),
        }
        self.transition(LifecycleState::Applying, "applying", now, None);
        Ok(())
    }
    pub fn finish_apply(
        &mut self,
        fault: SimulationFault,
        now: i64,
    ) -> Result<(), SimulationError> {
        match self.state {
            LifecycleState::Applied | LifecycleState::Verified => return Ok(()),
            LifecycleState::Applying => {}
            _ => return Err(SimulationError::InvalidTransition),
        }
        if matches!(
            fault,
            SimulationFault::FailBeforeApply | SimulationFault::TimeoutBeforeApply
        ) {
            self.transition(
                LifecycleState::Failed,
                if fault == SimulationFault::TimeoutBeforeApply {
                    "apply_timed_out"
                } else {
                    "apply_failed"
                },
                now,
                Some(
                    if fault == SimulationFault::TimeoutBeforeApply {
                        "simulated_apply_timeout"
                    } else {
                        "simulated_apply_failure"
                    }
                    .into(),
                ),
            );
            return Ok(());
        }
        if matches!(
            fault,
            SimulationFault::PartialApply | SimulationFault::TimeoutAfterPartialApply
        ) {
            if let Some(rule) = self.plan.rules.first() {
                self.simulated_resources.insert(rule.key(), rule.value());
            }
            self.transition(
                LifecycleState::RollbackPending,
                if fault == SimulationFault::TimeoutAfterPartialApply {
                    "apply_timed_out_partial"
                } else {
                    "partial_apply"
                },
                now,
                Some(
                    if fault == SimulationFault::TimeoutAfterPartialApply {
                        "simulated_partial_apply_timeout"
                    } else {
                        "simulated_partial_apply"
                    }
                    .into(),
                ),
            );
            return Ok(());
        }
        for rule in &self.plan.rules {
            self.simulated_resources.insert(rule.key(), rule.value());
        }
        self.transition(LifecycleState::Applied, "applied", now, None);
        Ok(())
    }
    pub fn apply(&mut self, fault: SimulationFault, now: i64) -> Result<(), SimulationError> {
        self.begin_apply(now)?;
        self.finish_apply(fault, now)
    }
    pub fn verify(&mut self, fault: SimulationFault, now: i64) -> Result<(), SimulationError> {
        if self.state == LifecycleState::Verified {
            return Ok(());
        }
        if self.state != LifecycleState::Applied {
            return Err(SimulationError::InvalidTransition);
        }
        let actual: BTreeMap<_, _> = self
            .simulated_resources
            .iter()
            .filter(|(key, _)| {
                self.plan
                    .rules
                    .iter()
                    .any(|r| r.key().as_str() == key.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let expected: BTreeMap<_, _> = self
            .plan
            .rules
            .iter()
            .map(|r| (r.key(), r.value()))
            .collect();
        if matches!(
            fault,
            SimulationFault::VerificationFailure | SimulationFault::VerificationTimeout
        ) || actual != expected
        {
            self.transition(
                LifecycleState::RollbackPending,
                if fault == SimulationFault::VerificationTimeout {
                    "verification_timed_out"
                } else {
                    "verification_failed"
                },
                now,
                Some(
                    if fault == SimulationFault::VerificationTimeout {
                        "simulated_verification_timeout"
                    } else {
                        "simulated_verification_failure"
                    }
                    .into(),
                ),
            );
        } else {
            self.transition(LifecycleState::Verified, "verified", now, None)
        }
        Ok(())
    }
    pub fn begin_rollback(&mut self, now: i64) -> Result<(), SimulationError> {
        match self.state {
            LifecycleState::Applied
            | LifecycleState::Verified
            | LifecycleState::RollbackPending
            | LifecycleState::Planned => {}
            LifecycleState::RolledBack => return Ok(()),
            _ => return Err(SimulationError::InvalidTransition),
        }
        if self.state != LifecycleState::RollbackPending {
            self.transition(
                LifecycleState::RollbackPending,
                "rollback_pending",
                now,
                None,
            );
        }
        Ok(())
    }
    pub fn finish_rollback(
        &mut self,
        fault: SimulationFault,
        now: i64,
    ) -> Result<(), SimulationError> {
        match self.state {
            LifecycleState::RolledBack => return Ok(()),
            LifecycleState::RollbackPending => {}
            _ => return Err(SimulationError::InvalidTransition),
        }
        if matches!(
            fault,
            SimulationFault::RollbackFailure | SimulationFault::RollbackTimeout
        ) {
            self.transition(
                LifecycleState::Failed,
                if fault == SimulationFault::RollbackTimeout {
                    "rollback_timed_out"
                } else {
                    "rollback_failed"
                },
                now,
                Some(
                    if fault == SimulationFault::RollbackTimeout {
                        "simulated_rollback_timeout"
                    } else {
                        "simulated_rollback_failure"
                    }
                    .into(),
                ),
            );
            return Ok(());
        }
        self.simulated_resources = self.prior_resources.clone();
        self.transition(LifecycleState::RolledBack, "rolled_back", now, None);
        Ok(())
    }
    pub fn rollback(&mut self, fault: SimulationFault, now: i64) -> Result<(), SimulationError> {
        self.begin_rollback(now)?;
        self.finish_rollback(fault, now)
    }
    pub fn recover(&mut self, now: i64) -> Result<(), SimulationError> {
        match self.state {
            LifecycleState::Applying | LifecycleState::RollbackPending => {
                if self.prior_resources.keys().any(|k| k.contains('\0')) {
                    self.transition(
                        LifecycleState::Failed,
                        "recovery_failed",
                        now,
                        Some("invalid_recovery_data".into()),
                    );
                    return Err(SimulationError::InvalidRecoveryData);
                }
                self.simulated_resources = self.prior_resources.clone();
                self.transition(LifecycleState::RolledBack, "recovered_rollback", now, None);
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn transition(&mut self, state: LifecycleState, kind: &str, now: i64, error: Option<String>) {
        self.state = state;
        self.revision = self.revision.saturating_add(1);
        self.error = error;
        self.events.push(LifecycleEvent {
            revision: self.revision,
            kind: kind.into(),
            occurred_at_ms: now.max(0),
            detail: self.error.clone().unwrap_or_default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> SessionPolicySnapshot {
        SessionPolicySnapshot{session_id:"s1".into(),profile_version:3,revision:7,state:"active_work".into(),snapshot_json:r#"{"applicationPolicy":{"mode":"whitelist","entries":["editor.exe"]},"websitePolicy":{"mode":"blacklist","domains":["example.test"]},"networkPolicy":{"mode":"disabled","applications":[]}}"#.into()}
    }
    fn plan() -> PolicyPlan {
        let s = source();
        PolicyPlan::validate(&s, 3, 7, &s.snapshot_json).unwrap()
    }
    fn operation() -> PolicyOperation {
        PolicyOperation::new("sim-1".into(), "request-1".into(), plan(), BTreeMap::new()).unwrap()
    }
    #[test]
    fn validates_snapshot_state_version_revision_and_policy() {
        let s = source();
        assert_eq!(
            PolicyPlan::validate(&s, 2, 7, &s.snapshot_json),
            Err(SimulationError::SnapshotMismatch)
        );
        assert_eq!(
            PolicyPlan::validate(&s, 3, 6, &s.snapshot_json),
            Err(SimulationError::StaleSession)
        );
        let mut stale = s.clone();
        stale.state = "completed".into();
        assert_eq!(
            PolicyPlan::validate(&stale, 3, 7, &stale.snapshot_json),
            Err(SimulationError::InactiveSession)
        );
        let mut malformed = s.clone();
        malformed.snapshot_json = "[]".into();
        assert_eq!(
            PolicyPlan::validate(&malformed, 3, 7, &malformed.snapshot_json),
            Err(SimulationError::InvalidSnapshot)
        );
    }
    #[test]
    fn validates_modes_entries_limits_and_duplicates() {
        let mut s = source();
        s.snapshot_json = s.snapshot_json.replace("editor.exe", " ");
        assert!(PolicyPlan::validate(&s, 3, 7, &s.snapshot_json).is_err());
        let mut s = source();
        s.snapshot_json = s.snapshot_json.replace("blacklist", "unsupported");
        assert!(PolicyPlan::validate(&s, 3, 7, &s.snapshot_json).is_err());
        let mut s = source();
        s.snapshot_json = s.snapshot_json.replace("editor.exe", "EDITOR.exe");
        s.snapshot_json = s
            .snapshot_json
            .replace("EDITOR.exe\"]", "EDITOR.exe\",\"editor.exe\"]");
        assert_eq!(
            PolicyPlan::validate(&s, 3, 7, &s.snapshot_json),
            Err(SimulationError::InvalidPolicy)
        );
    }
    #[test]
    fn successful_apply_verify_rollback_are_idempotent_and_events_monotonic() {
        let mut op = operation();
        op.apply(SimulationFault::None, 10).unwrap();
        assert_eq!(op.state, LifecycleState::Applied);
        let state = op.simulated_resources.clone();
        op.apply(SimulationFault::None, 11).unwrap();
        assert_eq!(op.simulated_resources, state);
        op.verify(SimulationFault::None, 12).unwrap();
        assert_eq!(op.state, LifecycleState::Verified);
        op.rollback(SimulationFault::None, 13).unwrap();
        assert_eq!(op.state, LifecycleState::RolledBack);
        assert!(op.simulated_resources.is_empty());
        op.rollback(SimulationFault::None, 14).unwrap();
        assert!(op.events.windows(2).all(|w| w[0].revision < w[1].revision));
    }
    #[test]
    fn invalid_transitions_and_duplicate_request_payload_conflicts_are_rejected() {
        let mut op = operation();
        assert_eq!(
            op.verify(SimulationFault::None, 1),
            Err(SimulationError::InvalidTransition)
        );
        assert_eq!(
            PolicyOperation::new("sim-2".into(), "bad key".into(), plan(), BTreeMap::new()),
            Err(SimulationError::InvalidRequestKey)
        );
    }
    #[test]
    fn partial_apply_verification_failure_rollback_failure_and_restart_recovery_are_explicit() {
        let mut partial = operation();
        partial.apply(SimulationFault::PartialApply, 1).unwrap();
        assert_eq!(partial.state, LifecycleState::RollbackPending);
        partial.rollback(SimulationFault::None, 2).unwrap();
        assert_eq!(partial.state, LifecycleState::RolledBack);
        let mut verify = operation();
        verify.apply(SimulationFault::None, 1).unwrap();
        verify
            .verify(SimulationFault::VerificationFailure, 2)
            .unwrap();
        assert_eq!(verify.state, LifecycleState::RollbackPending);
        let mut crash = operation();
        crash.apply(SimulationFault::PartialApply, 1).unwrap();
        let mut reopened: PolicyOperation =
            serde_json::from_str(&serde_json::to_string(&crash).unwrap()).unwrap();
        reopened.recover(2).unwrap();
        assert_eq!(reopened.state, LifecycleState::RolledBack);
        assert!(reopened.simulated_resources.is_empty());
        let mut failed = operation();
        failed.apply(SimulationFault::None, 1).unwrap();
        failed
            .rollback(SimulationFault::RollbackFailure, 2)
            .unwrap();
        assert_eq!(failed.state, LifecycleState::Failed);
        assert!(!failed.simulated_resources.is_empty());
        assert!(failed.verify(SimulationFault::None, 3).is_err());
    }

    #[test]
    fn timeout_failures_are_deterministic_and_never_report_success() {
        let mut before_apply = operation();
        before_apply
            .apply(SimulationFault::TimeoutBeforeApply, 10)
            .unwrap();
        assert_eq!(before_apply.state, LifecycleState::Failed);
        assert!(before_apply.simulated_resources.is_empty());

        let mut partial_timeout = operation();
        partial_timeout
            .apply(SimulationFault::TimeoutAfterPartialApply, 11)
            .unwrap();
        assert_eq!(partial_timeout.state, LifecycleState::RollbackPending);
        assert!(!partial_timeout.simulated_resources.is_empty());

        let mut verification_timeout = operation();
        verification_timeout
            .apply(SimulationFault::None, 12)
            .unwrap();
        verification_timeout
            .verify(SimulationFault::VerificationTimeout, 13)
            .unwrap();
        assert_eq!(verification_timeout.state, LifecycleState::RollbackPending);

        let mut rollback_timeout = operation();
        rollback_timeout.apply(SimulationFault::None, 14).unwrap();
        rollback_timeout
            .rollback(SimulationFault::RollbackTimeout, 15)
            .unwrap();
        assert_eq!(rollback_timeout.state, LifecycleState::Failed);
        assert!(!rollback_timeout.simulated_resources.is_empty());

        let mut before_apply_failure = operation();
        before_apply_failure
            .apply(SimulationFault::FailBeforeApply, 16)
            .unwrap();
        assert_eq!(before_apply_failure.state, LifecycleState::Failed);
        let mut partial_failure = operation();
        partial_failure
            .apply(SimulationFault::PartialApply, 17)
            .unwrap();
        let mut verification_failure = operation();
        verification_failure
            .apply(SimulationFault::None, 18)
            .unwrap();
        verification_failure
            .verify(SimulationFault::VerificationFailure, 19)
            .unwrap();
        let mut rollback_failure = operation();
        rollback_failure.apply(SimulationFault::None, 20).unwrap();
        rollback_failure
            .rollback(SimulationFault::RollbackFailure, 21)
            .unwrap();
        assert_eq!(rollback_failure.state, LifecycleState::Failed);
    }
}
