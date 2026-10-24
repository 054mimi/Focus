use serde::{Deserialize, Serialize};

const MIN_PROFILE_MINUTES: u32 = 5;
const MAX_PROFILE_MINUTES: u32 = 1_440;
const MINUTE_MS: i64 = 60_000;
const SECOND_MS: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    ActiveWork,
    ActiveBreak,
    AwaitingCompletion,
    Completed,
}

impl SessionState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::ActiveWork | Self::ActiveBreak)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionPolicy {
    TimerOnly,
    TaskConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOutcome {
    Completed,
    Partial,
    NotCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedBreak {
    pub duration_minutes: u32,
}

#[derive(Debug, Clone)]
pub struct VerifiedProfile {
    pub profile_id: String,
    pub profile_version: i64,
    pub visible: bool,
    pub approved: bool,
    pub snapshot_json: String,
    pub duration_minutes: u32,
    pub completion_policy: CompletionPolicy,
    pub planned_breaks: Vec<PlannedBreak>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockReading {
    pub wall_utc_ms: i64,
    pub monotonic_ms: u64,
}

/// Monotonic readings are meaningful only for this process lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockAnchor {
    pub wall_utc_ms: i64,
    pub monotonic_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Started,
    TimeShortened,
    BreakStarted,
    BreakEnded,
    BreakSkipped,
    DeadlineExpired,
    TaskOutcomeRecorded,
    ClockDiscrepancy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    pub kind: EventKind,
    pub occurred_at_ms: i64,
    pub revision: u64,
    pub detail_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    NotConfirmed,
    InvalidProfile,
    InvalidClock,
    TimeOverflow,
    AlreadyExpired,
    InvalidAdjustment,
    ExtensionNotAllowed,
    ShorteningNotAllowed,
    BelowMinimumRemaining,
    StaleRevision,
    InvalidTransition,
    NoBreakRemaining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub profile_id: String,
    pub profile_version: i64,
    pub snapshot_json: String,
    pub planned_duration_minutes: u32,
    pub started_at_ms: i64,
    pub deadline_at_ms: i64,
    pub state: SessionState,
    pub completion_policy: CompletionPolicy,
    pub completion_outcome: Option<CompletionOutcome>,
    pub break_index: u32,
    pub break_deadline_at_ms: Option<i64>,
    pub expired_at_ms: Option<i64>,
    pub revision: u64,
    pub clock_anomaly_delta_ms: Option<i64>,
}

impl Session {
    pub fn start(
        id: String,
        profile: VerifiedProfile,
        confirmed: bool,
        clock: ClockReading,
    ) -> Result<(Self, SessionEvent), DomainError> {
        if !confirmed {
            return Err(DomainError::NotConfirmed);
        }
        if !profile.visible
            || !profile.approved
            || profile.profile_version <= 0
            || id.is_empty()
            || serde_json::from_str::<serde_json::Value>(&profile.snapshot_json)
                .ok()
                .is_none_or(|value| !value.is_object())
            || !(MIN_PROFILE_MINUTES..=MAX_PROFILE_MINUTES).contains(&profile.duration_minutes)
            || profile.planned_breaks.len() > 8
            || profile
                .planned_breaks
                .iter()
                .any(|b| !(1..=180).contains(&b.duration_minutes))
        {
            return Err(DomainError::InvalidProfile);
        }
        if clock.wall_utc_ms < 0 {
            return Err(DomainError::InvalidClock);
        }
        let duration_ms = i64::from(profile.duration_minutes)
            .checked_mul(MINUTE_MS)
            .ok_or(DomainError::TimeOverflow)?;
        let deadline_at_ms = clock
            .wall_utc_ms
            .checked_add(duration_ms)
            .ok_or(DomainError::TimeOverflow)?;
        let session = Self {
            id,
            profile_id: profile.profile_id,
            profile_version: profile.profile_version,
            snapshot_json: profile.snapshot_json,
            planned_duration_minutes: profile.duration_minutes,
            started_at_ms: clock.wall_utc_ms,
            deadline_at_ms,
            state: SessionState::ActiveWork,
            completion_policy: profile.completion_policy,
            completion_outcome: None,
            break_index: 0,
            break_deadline_at_ms: None,
            expired_at_ms: None,
            revision: 0,
            clock_anomaly_delta_ms: None,
        };
        let event = session.event(EventKind::Started, clock.wall_utc_ms, "{}");
        Ok((session, event))
    }

    /// While running, choose the greater of wall and monotonic elapsed time.
    /// After restart, the caller anchors both values at process startup so UTC
    /// deadline recovery remains the only cross-process source of elapsed time.
    pub fn effective_now_ms(
        anchor: ClockAnchor,
        reading: ClockReading,
    ) -> Result<(i64, Option<i64>), DomainError> {
        if anchor.wall_utc_ms < 0 || reading.wall_utc_ms < 0 {
            return Err(DomainError::InvalidClock);
        }
        let wall_elapsed = reading
            .wall_utc_ms
            .saturating_sub(anchor.wall_utc_ms)
            .max(0);
        let monotonic_elapsed = reading
            .monotonic_ms
            .saturating_sub(anchor.monotonic_ms)
            .min(i64::MAX as u64) as i64;
        let elapsed = wall_elapsed.max(monotonic_elapsed);
        let effective = anchor
            .wall_utc_ms
            .checked_add(elapsed)
            .ok_or(DomainError::TimeOverflow)?;
        let delta = wall_elapsed.saturating_sub(monotonic_elapsed);
        let anomaly = (delta.unsigned_abs() > SECOND_MS as u64).then_some(delta);
        Ok((effective, anomaly))
    }

    pub fn remaining_ms(&self, now_ms: i64) -> i64 {
        self.deadline_at_ms.saturating_sub(now_ms).max(0)
    }

    pub fn reconcile(
        &mut self,
        anchor: ClockAnchor,
        reading: ClockReading,
    ) -> Result<Vec<SessionEvent>, DomainError> {
        let (now, anomaly) = Self::effective_now_ms(anchor, reading)?;
        let mut events = Vec::new();
        if let Some(delta) = anomaly {
            if self.clock_anomaly_delta_ms != Some(delta) {
                self.clock_anomaly_delta_ms = Some(delta);
                self.revision = self.revision.saturating_add(1);
                events.push(self.event(
                    EventKind::ClockDiscrepancy,
                    now,
                    &format!("{{\"wallMinusMonotonicMs\":{delta}}}"),
                ));
            }
        }
        if !self.state.is_active() {
            return Ok(events);
        }
        if now >= self.deadline_at_ms {
            self.state = match self.completion_policy {
                CompletionPolicy::TimerOnly => SessionState::Completed,
                CompletionPolicy::TaskConfirmation => SessionState::AwaitingCompletion,
            };
            self.break_deadline_at_ms = None;
            self.expired_at_ms = Some(self.deadline_at_ms);
            self.revision = self.revision.saturating_add(1);
            events.push(self.event(EventKind::DeadlineExpired, self.deadline_at_ms, "{}"));
        } else if self.state == SessionState::ActiveBreak
            && self
                .break_deadline_at_ms
                .is_some_and(|deadline| now >= deadline)
        {
            self.state = SessionState::ActiveWork;
            self.break_deadline_at_ms = None;
            self.revision = self.revision.saturating_add(1);
            events.push(self.event(EventKind::BreakEnded, now, "{}"));
        }
        Ok(events)
    }

    pub fn shorten_to_minutes(
        &mut self,
        expected_revision: u64,
        new_remaining_minutes: u32,
        anchor: ClockAnchor,
        reading: ClockReading,
    ) -> Result<Vec<SessionEvent>, DomainError> {
        let mut events = self.reconcile(anchor, reading)?;
        if self.revision != expected_revision {
            return Err(DomainError::StaleRevision);
        }
        if !self.state.is_active() {
            return Err(DomainError::AlreadyExpired);
        }
        let (now, _) = Self::effective_now_ms(anchor, reading)?;
        let remaining = self.remaining_ms(now);
        let requested = i64::from(new_remaining_minutes)
            .checked_mul(MINUTE_MS)
            .ok_or(DomainError::InvalidAdjustment)?;
        if requested <= 0 {
            return Err(DomainError::InvalidAdjustment);
        }
        if requested == remaining {
            return Ok(events);
        }
        if requested > remaining {
            return Err(DomainError::ExtensionNotAllowed);
        }
        let minimum_minutes = if remaining > 60 * MINUTE_MS {
            let half_ms = remaining.checked_add(1).ok_or(DomainError::TimeOverflow)? / 2;
            u32::try_from((half_ms + MINUTE_MS - 1) / MINUTE_MS)
                .map_err(|_| DomainError::TimeOverflow)?
        } else if remaining >= 30 * MINUTE_MS {
            30
        } else if remaining > 15 * MINUTE_MS {
            15
        } else {
            return Err(DomainError::ShorteningNotAllowed);
        };
        if new_remaining_minutes < minimum_minutes {
            return Err(DomainError::BelowMinimumRemaining);
        }
        let previous_deadline = self.deadline_at_ms;
        self.deadline_at_ms = now
            .checked_add(requested)
            .ok_or(DomainError::TimeOverflow)?;
        if let Some(break_deadline) = self.break_deadline_at_ms {
            self.break_deadline_at_ms = Some(break_deadline.min(self.deadline_at_ms));
        }
        self.revision = self.revision.saturating_add(1);
        events.push(self.event(
            EventKind::TimeShortened,
            now,
            &format!(
                "{{\"previousDeadlineMs\":{previous_deadline},\"newDeadlineMs\":{}}}",
                self.deadline_at_ms
            ),
        ));
        Ok(events)
    }

    pub fn start_next_break(
        &mut self,
        breaks: &[PlannedBreak],
        expected_revision: u64,
        anchor: ClockAnchor,
        reading: ClockReading,
    ) -> Result<Vec<SessionEvent>, DomainError> {
        let mut events = self.reconcile(anchor, reading)?;
        if self.revision != expected_revision {
            return Err(DomainError::StaleRevision);
        }
        if self.state != SessionState::ActiveWork {
            return Err(DomainError::InvalidTransition);
        }
        let planned = breaks
            .get(self.break_index as usize)
            .ok_or(DomainError::NoBreakRemaining)?;
        let (now, _) = Self::effective_now_ms(anchor, reading)?;
        if now >= self.deadline_at_ms {
            return Err(DomainError::AlreadyExpired);
        }
        let break_duration = i64::from(planned.duration_minutes)
            .checked_mul(MINUTE_MS)
            .ok_or(DomainError::TimeOverflow)?;
        self.break_deadline_at_ms = Some(
            now.checked_add(break_duration)
                .ok_or(DomainError::TimeOverflow)?
                .min(self.deadline_at_ms),
        );
        self.break_index = self.break_index.saturating_add(1);
        self.state = SessionState::ActiveBreak;
        self.revision = self.revision.saturating_add(1);
        events.push(self.event(EventKind::BreakStarted, now, "{}"));
        Ok(events)
    }

    pub fn skip_break(
        &mut self,
        expected_revision: u64,
        anchor: ClockAnchor,
        reading: ClockReading,
    ) -> Result<Vec<SessionEvent>, DomainError> {
        let mut events = self.reconcile(anchor, reading)?;
        if self.revision != expected_revision {
            return Err(DomainError::StaleRevision);
        }
        if self.state != SessionState::ActiveBreak {
            return Err(DomainError::InvalidTransition);
        }
        let (now, _) = Self::effective_now_ms(anchor, reading)?;
        self.state = SessionState::ActiveWork;
        self.break_deadline_at_ms = None;
        self.revision = self.revision.saturating_add(1);
        events.push(self.event(EventKind::BreakSkipped, now, "{}"));
        Ok(events)
    }

    pub fn record_outcome(
        &mut self,
        expected_revision: u64,
        outcome: CompletionOutcome,
        now_utc_ms: i64,
    ) -> Result<SessionEvent, DomainError> {
        if self.revision != expected_revision {
            return Err(DomainError::StaleRevision);
        }
        if self.state != SessionState::AwaitingCompletion || now_utc_ms < 0 {
            return Err(DomainError::InvalidTransition);
        }
        self.completion_outcome = Some(outcome);
        self.state = SessionState::Completed;
        self.revision = self.revision.saturating_add(1);
        Ok(self.event(
            EventKind::TaskOutcomeRecorded,
            now_utc_ms,
            &format!(
                "{{\"outcome\":\"{}\"}}",
                serde_json::to_value(outcome).unwrap().as_str().unwrap()
            ),
        ))
    }

    fn event(&self, kind: EventKind, occurred_at_ms: i64, detail_json: &str) -> SessionEvent {
        SessionEvent {
            kind,
            occurred_at_ms,
            revision: self.revision,
            detail_json: detail_json.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const ANCHOR: ClockAnchor = ClockAnchor {
        wall_utc_ms: NOW,
        monotonic_ms: 10_000,
    };
    fn profile(duration_minutes: u32) -> VerifiedProfile {
        VerifiedProfile {
            profile_id: "profile-1".into(),
            profile_version: 2,
            visible: true,
            approved: true,
            snapshot_json: r#"{"activity":"Code","intendedOutcome":"Complete feature"}"#.into(),
            duration_minutes,
            completion_policy: CompletionPolicy::TimerOnly,
            planned_breaks: vec![],
        }
    }
    fn session(duration: u32) -> Session {
        Session::start(
            "session-1".into(),
            profile(duration),
            true,
            ClockReading {
                wall_utc_ms: NOW,
                monotonic_ms: 10_000,
            },
        )
        .unwrap()
        .0
    }

    #[test]
    fn start_requires_confirmation_and_valid_approved_visible_snapshot() {
        assert_eq!(
            Session::start(
                "s".into(),
                profile(45),
                false,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 0
                }
            )
            .unwrap_err(),
            DomainError::NotConfirmed
        );
        let mut hidden = profile(45);
        hidden.visible = false;
        assert_eq!(
            Session::start(
                "s".into(),
                hidden,
                true,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 0
                }
            )
            .unwrap_err(),
            DomainError::InvalidProfile
        );
        let mut unapproved = profile(45);
        unapproved.approved = false;
        assert_eq!(
            Session::start(
                "s".into(),
                unapproved,
                true,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 0
                }
            )
            .unwrap_err(),
            DomainError::InvalidProfile
        );
        assert!(
            Session::start(
                "s".into(),
                profile(45),
                true,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 0
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn all_required_shortening_examples_use_current_remaining_time() {
        for (from, to) in [(120, 60), (90, 45), (60, 30), (45, 30), (30, 30), (20, 15)] {
            let mut s = session(from);
            let result = s.shorten_to_minutes(
                0,
                to,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000,
                },
            );
            if from == 30 {
                assert!(result.is_ok());
            } else {
                assert!(result.is_ok(), "{from}->{to}");
            }
        }
        for from in [15, 10] {
            let mut s = session(from);
            assert!(
                s.shorten_to_minutes(
                    0,
                    from,
                    ANCHOR,
                    ClockReading {
                        wall_utc_ms: NOW,
                        monotonic_ms: 10_000
                    }
                )
                .is_ok()
            );
            let mut s = session(from);
            assert_eq!(
                s.shorten_to_minutes(
                    0,
                    from - 1,
                    ANCHOR,
                    ClockReading {
                        wall_utc_ms: NOW,
                        monotonic_ms: 10_000
                    }
                )
                .unwrap_err(),
                DomainError::ShorteningNotAllowed
            );
        }
    }

    #[test]
    fn shortening_boundaries_rounding_and_invalid_requests_are_rejected_safely() {
        let mut s = session(61);
        assert_eq!(
            s.shorten_to_minutes(
                0,
                30,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::BelowMinimumRemaining
        );
        assert!(
            s.shorten_to_minutes(
                0,
                31,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .is_ok()
        );
        let mut s = session(60);
        assert_eq!(
            s.shorten_to_minutes(
                0,
                29,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::BelowMinimumRemaining
        );
        let mut s = session(30);
        assert!(
            s.shorten_to_minutes(
                0,
                30,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .is_ok()
        );
        let mut s = session(30);
        assert_eq!(
            s.shorten_to_minutes(
                0,
                29,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::BelowMinimumRemaining
        );
        let mut s = session(20);
        assert_eq!(
            s.shorten_to_minutes(
                0,
                14,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::BelowMinimumRemaining
        );
        for target in [0, u32::MAX] {
            let mut s = session(45);
            assert!(
                s.shorten_to_minutes(
                    0,
                    target,
                    ANCHOR,
                    ClockReading {
                        wall_utc_ms: NOW,
                        monotonic_ms: 10_000
                    }
                )
                .is_err()
            );
        }
        let mut s = session(45);
        assert!(
            s.shorten_to_minutes(
                0,
                45,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .is_ok()
        );
        assert_eq!(
            s.shorten_to_minutes(
                0,
                46,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::ExtensionNotAllowed
        );
    }

    #[test]
    fn repeated_and_stale_adjustments_cannot_reuse_an_old_boundary() {
        let mut s = session(120);
        s.shorten_to_minutes(
            0,
            60,
            ANCHOR,
            ClockReading {
                wall_utc_ms: NOW,
                monotonic_ms: 10_000,
            },
        )
        .unwrap();
        assert_eq!(
            s.shorten_to_minutes(
                0,
                30,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::StaleRevision
        );
        assert!(
            s.shorten_to_minutes(
                1,
                30,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn expiry_is_exactly_once_and_task_confirmation_is_never_automatic() {
        let mut s = session(5);
        s.completion_policy = CompletionPolicy::TaskConfirmation;
        let after = ClockReading {
            wall_utc_ms: NOW + 5 * MINUTE_MS,
            monotonic_ms: 10_000 + (5 * MINUTE_MS) as u64,
        };
        let first = s.reconcile(ANCHOR, after).unwrap();
        assert_eq!(s.state, SessionState::AwaitingCompletion);
        assert_eq!(
            first
                .iter()
                .filter(|e| e.kind == EventKind::DeadlineExpired)
                .count(),
            1
        );
        let second = s.reconcile(ANCHOR, after).unwrap();
        assert_eq!(
            second
                .iter()
                .filter(|e| e.kind == EventKind::DeadlineExpired)
                .count(),
            0
        );
        assert_eq!(
            s.record_outcome(s.revision, CompletionOutcome::Partial, after.wall_utc_ms)
                .unwrap()
                .kind,
            EventKind::TaskOutcomeRecorded
        );
        assert_eq!(s.state, SessionState::Completed);
        assert_eq!(
            s.record_outcome(s.revision, CompletionOutcome::Completed, after.wall_utc_ms)
                .unwrap_err(),
            DomainError::InvalidTransition
        );
    }

    #[test]
    fn breaks_are_bounded_manual_and_expiry_precedes_break_end() {
        let breaks = vec![PlannedBreak {
            duration_minutes: 10,
        }];
        let mut s = session(5);
        assert!(
            s.start_next_break(
                &breaks,
                0,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .is_ok()
        );
        assert_eq!(s.break_deadline_at_ms, Some(s.deadline_at_ms));
        assert_eq!(
            s.start_next_break(
                &breaks,
                1,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::InvalidTransition
        );
        let after = ClockReading {
            wall_utc_ms: NOW + 6 * MINUTE_MS,
            monotonic_ms: 10_000 + (6 * MINUTE_MS) as u64,
        };
        let events = s.reconcile(ANCHOR, after).unwrap();
        assert_eq!(s.state, SessionState::Completed);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == EventKind::DeadlineExpired)
                .count(),
            1
        );
        let mut s = session(45);
        assert_eq!(
            s.start_next_break(
                &[],
                0,
                ANCHOR,
                ClockReading {
                    wall_utc_ms: NOW,
                    monotonic_ms: 10_000
                }
            )
            .unwrap_err(),
            DomainError::NoBreakRemaining
        );
        s.start_next_break(
            &breaks,
            0,
            ANCHOR,
            ClockReading {
                wall_utc_ms: NOW,
                monotonic_ms: 10_000,
            },
        )
        .unwrap();
        s.skip_break(
            1,
            ANCHOR,
            ClockReading {
                wall_utc_ms: NOW,
                monotonic_ms: 10_000,
            },
        )
        .unwrap();
        assert_eq!(s.state, SessionState::ActiveWork);
        assert_eq!(s.break_deadline_at_ms, None);
    }

    #[test]
    fn wall_clock_changes_and_recovery_have_explicit_deterministic_behavior() {
        let backward = ClockReading {
            wall_utc_ms: NOW - 10 * MINUTE_MS,
            monotonic_ms: 10_000 + (2 * MINUTE_MS) as u64,
        };
        let (now, anomaly) = Session::effective_now_ms(ANCHOR, backward).unwrap();
        assert_eq!(now, NOW + 2 * MINUTE_MS);
        assert!(anomaly.is_some());
        let forward = ClockReading {
            wall_utc_ms: NOW + 7 * MINUTE_MS,
            monotonic_ms: 10_000 + (2 * MINUTE_MS) as u64,
        };
        assert_eq!(
            Session::effective_now_ms(ANCHOR, forward).unwrap().0,
            NOW + 7 * MINUTE_MS
        );
        let reboot_anchor = ClockAnchor {
            wall_utc_ms: NOW + 3 * MINUTE_MS,
            monotonic_ms: 0,
        };
        assert_eq!(
            Session::effective_now_ms(
                reboot_anchor,
                ClockReading {
                    wall_utc_ms: NOW + 3 * MINUTE_MS,
                    monotonic_ms: 0
                }
            )
            .unwrap()
            .0,
            NOW + 3 * MINUTE_MS
        );
    }
}
