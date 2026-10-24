use crate::{
    policy_simulation::PolicyOperation,
    policy_simulation_store,
    session_domain::{
        ClockAnchor, ClockReading, CompletionOutcome, CompletionPolicy, DomainError, PlannedBreak,
        Session, SessionEvent, SessionState, VerifiedProfile,
    },
    storage,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) struct AppDatabase {
    pub(crate) connection: Mutex<Connection>,
    clock: Mutex<RuntimeClock>,
}

struct RuntimeClock {
    process_origin: Instant,
    active_anchor: Option<(String, ClockAnchor)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionView {
    id: String,
    profile_id: String,
    profile_version: i64,
    snapshot: serde_json::Value,
    planned_duration_minutes: u32,
    started_at_ms: i64,
    deadline_at_ms: i64,
    state: SessionState,
    completion_policy: CompletionPolicy,
    completion_outcome: Option<CompletionOutcome>,
    break_index: u32,
    break_deadline_at_ms: Option<i64>,
    remaining_ms: i64,
    break_remaining_ms: i64,
    revision: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionStatus {
    active: Option<SessionView>,
    awaiting_completion: Vec<SessionView>,
}

impl AppDatabase {
    pub(crate) fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        storage::configure_connection(&connection)?;
        let mut connection = connection;
        storage::migrate(&mut connection)?;
        policy_simulation_store::backfill_requests(
            &mut connection,
            policy_simulation_store::utc_now_ms()?,
        )?;
        policy_simulation_store::recover_incomplete(
            &mut connection,
            policy_simulation_store::utc_now_ms()?,
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
            clock: Mutex::new(RuntimeClock {
                process_origin: Instant::now(),
                active_anchor: None,
            }),
        })
    }

    fn reading(clock: &RuntimeClock) -> Result<ClockReading, String> {
        let wall_utc_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let wall_utc_ms = i64::try_from(wall_utc_ms).map_err(|e| e.to_string())?;
        let monotonic_ms =
            u64::try_from(clock.process_origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(ClockReading {
            wall_utc_ms,
            monotonic_ms,
        })
    }

    fn anchor_for(
        clock: &mut RuntimeClock,
        session: &Session,
        reading: ClockReading,
    ) -> ClockAnchor {
        if let Some((id, anchor)) = &clock.active_anchor {
            if id == &session.id {
                return *anchor;
            }
        }
        let anchor = ClockAnchor {
            wall_utc_ms: reading.wall_utc_ms,
            monotonic_ms: reading.monotonic_ms,
        };
        clock.active_anchor = Some((session.id.clone(), anchor));
        anchor
    }

    fn update_anchor(clock: &mut RuntimeClock, session: &Session, reading: ClockReading) {
        if session.state.is_active() {
            if clock
                .active_anchor
                .as_ref()
                .is_none_or(|(id, _)| id != &session.id)
            {
                clock.active_anchor = Some((
                    session.id.clone(),
                    ClockAnchor {
                        wall_utc_ms: reading.wall_utc_ms,
                        monotonic_ms: reading.monotonic_ms,
                    },
                ));
            }
        } else if clock
            .active_anchor
            .as_ref()
            .is_some_and(|(id, _)| id == &session.id)
        {
            clock.active_anchor = None;
        }
    }

    pub(crate) fn status(&self) -> Result<SessionStatus, String> {
        let mut connection = self.connection.lock().map_err(|e| e.to_string())?;
        let mut clock = self.clock.lock().map_err(|e| e.to_string())?;
        let reading = Self::reading(&clock)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let mut active = load_active(&tx)?;
        if let Some(session) = active.as_mut() {
            let anchor = Self::anchor_for(&mut clock, session, reading);
            let events = session.reconcile(anchor, reading).map_err(domain_error)?;
            if !events.is_empty() {
                persist_session(&tx, session)?;
                persist_events(&tx, &session.id, &events)?;
            }
            Self::update_anchor(&mut clock, session, reading);
        }
        let pending = load_pending(&tx)?;
        let active_view = active
            .as_ref()
            .filter(|s| s.state.is_active())
            .map(|s| view(s, reading, active_anchor(&clock, s, reading)));
        let pending_views = pending
            .iter()
            .map(|s| {
                view(
                    s,
                    reading,
                    ClockAnchor {
                        wall_utc_ms: reading.wall_utc_ms,
                        monotonic_ms: reading.monotonic_ms,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(SessionStatus {
            active: active_view.transpose()?,
            awaiting_completion: pending_views,
        })
    }

    pub(crate) fn start(
        &self,
        profile_id: String,
        profile_version: i64,
        confirmed: bool,
    ) -> Result<SessionView, String> {
        if !confirmed {
            return Err(domain_error(DomainError::NotConfirmed));
        }
        let mut connection = self.connection.lock().map_err(|e| e.to_string())?;
        let mut clock = self.clock.lock().map_err(|e| e.to_string())?;
        let reading = Self::reading(&clock)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if let Some(mut current) = load_active(&tx)? {
            let anchor = Self::anchor_for(&mut clock, &current, reading);
            let events = current.reconcile(anchor, reading).map_err(domain_error)?;
            if !events.is_empty() {
                persist_session(&tx, &current)?;
                persist_events(&tx, &current.id, &events)?;
            }
            if current.state.is_active() {
                tx.commit().map_err(|e| e.to_string())?;
                return Err("A session is already active.".into());
            }
            Self::update_anchor(&mut clock, &current, reading);
        }
        let profile = tx.query_row(
            "SELECT p.visible,p.approved_version,v.status,v.config_json FROM profiles p JOIN profile_versions v ON v.profile_id=p.id WHERE p.id=? AND v.version=?",
            params![profile_id, profile_version],
            |row| Ok((row.get::<_,bool>(0)?,row.get::<_,Option<i64>>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?)),
        ).optional().map_err(|e|e.to_string())?.ok_or("The selected profile version no longer exists.")?;
        if !profile.0 {
            return Err("The selected profile is hidden.".into());
        }
        if profile.1 != Some(profile_version) || profile.2 != "approved" {
            return Err("The selected profile version is no longer approved.".into());
        }
        let config: serde_json::Value =
            serde_json::from_str(&profile.3).map_err(|e| e.to_string())?;
        crate::validate_profile_config(&config)?;
        let completion_policy: CompletionPolicy = serde_json::from_value(
            config
                .get("completionPolicy")
                .cloned()
                .ok_or("Missing completion policy.")?,
        )
        .map_err(|e| e.to_string())?;
        let planned_breaks: Vec<PlannedBreak> = serde_json::from_value(
            config
                .get("plannedBreaks")
                .cloned()
                .ok_or("Missing planned breaks.")?,
        )
        .map_err(|e| e.to_string())?;
        let duration_minutes = u32::try_from(
            config
                .get("durationMinutes")
                .and_then(|v| v.as_u64())
                .ok_or("Invalid duration.")?,
        )
        .map_err(|e| e.to_string())?;
        let id_suffix: String = tx
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        let verified = VerifiedProfile {
            profile_id,
            profile_version,
            visible: profile.0,
            approved: true,
            snapshot_json: profile.3,
            duration_minutes,
            completion_policy,
            planned_breaks,
        };
        let (session, event) =
            Session::start(format!("session-{id_suffix}"), verified, confirmed, reading)
                .map_err(domain_error)?;
        insert_session(&tx, &session)?;
        persist_events(&tx, &session.id, &[event])?;
        tx.commit().map_err(|e| e.to_string())?;
        clock.active_anchor = Some((
            session.id.clone(),
            ClockAnchor {
                wall_utc_ms: reading.wall_utc_ms,
                monotonic_ms: reading.monotonic_ms,
            },
        ));
        view(
            &session,
            reading,
            ClockAnchor {
                wall_utc_ms: reading.wall_utc_ms,
                monotonic_ms: reading.monotonic_ms,
            },
        )
    }

    pub(crate) fn shorten(
        &self,
        id: String,
        expected_revision: u64,
        remaining_minutes: u32,
    ) -> Result<SessionView, String> {
        self.mutate(id, |session, anchor, reading| {
            session.shorten_to_minutes(expected_revision, remaining_minutes, anchor, reading)
        })
    }
    pub(crate) fn start_break(
        &self,
        id: String,
        expected_revision: u64,
    ) -> Result<SessionView, String> {
        self.mutate(id, |session, anchor, reading| {
            let breaks: Vec<PlannedBreak> =
                serde_json::from_str::<serde_json::Value>(&session.snapshot_json)
                    .ok()
                    .and_then(|v| v.get("plannedBreaks").cloned())
                    .and_then(|v| serde_json::from_value::<Vec<PlannedBreak>>(v).ok())
                    .unwrap_or_default();
            session.start_next_break(&breaks, expected_revision, anchor, reading)
        })
    }
    pub(crate) fn skip_break(
        &self,
        id: String,
        expected_revision: u64,
    ) -> Result<SessionView, String> {
        self.mutate(id, |session, anchor, reading| {
            session.skip_break(expected_revision, anchor, reading)
        })
    }
    pub(crate) fn record_outcome(
        &self,
        id: String,
        expected_revision: u64,
        outcome: CompletionOutcome,
    ) -> Result<SessionView, String> {
        self.mutate(id, |session, _anchor, reading| {
            session
                .record_outcome(expected_revision, outcome, reading.wall_utc_ms)
                .map(|event| vec![event])
        })
    }

    pub(crate) fn create_policy_simulation(
        &self,
        session_id: &str,
        expected_profile_version: i64,
        expected_revision: u64,
        request_id: &str,
    ) -> Result<PolicyOperation, String> {
        let mut c = self.connection.lock().map_err(|e| e.to_string())?;
        policy_simulation_store::create(
            &mut c,
            session_id,
            expected_profile_version,
            expected_revision,
            request_id,
            policy_simulation_store::utc_now_ms()?,
        )
    }
    pub(crate) fn get_policy_simulation(
        &self,
        operation_id: &str,
    ) -> Result<PolicyOperation, String> {
        let c = self.connection.lock().map_err(|e| e.to_string())?;
        policy_simulation_store::get(&c, operation_id)
    }
    pub(crate) fn apply_simulated_policy(
        &self,
        operation_id: &str,
        request_id: &str,
        expected_operation_revision: u64,
        expected_session_revision: u64,
    ) -> Result<PolicyOperation, String> {
        let mut c = self.connection.lock().map_err(|e| e.to_string())?;
        policy_simulation_store::apply(
            &mut c,
            operation_id,
            request_id,
            expected_operation_revision,
            expected_session_revision,
            policy_simulation_store::utc_now_ms()?,
        )
    }
    pub(crate) fn verify_simulated_policy(
        &self,
        operation_id: &str,
        request_id: &str,
        expected_operation_revision: u64,
        expected_session_revision: u64,
    ) -> Result<PolicyOperation, String> {
        let mut c = self.connection.lock().map_err(|e| e.to_string())?;
        policy_simulation_store::verify(
            &mut c,
            operation_id,
            request_id,
            expected_operation_revision,
            expected_session_revision,
            policy_simulation_store::utc_now_ms()?,
        )
    }
    pub(crate) fn rollback_simulated_policy(
        &self,
        operation_id: &str,
        request_id: &str,
        expected_operation_revision: u64,
    ) -> Result<PolicyOperation, String> {
        let mut c = self.connection.lock().map_err(|e| e.to_string())?;
        policy_simulation_store::rollback(
            &mut c,
            operation_id,
            request_id,
            expected_operation_revision,
            policy_simulation_store::utc_now_ms()?,
        )
    }

    fn mutate<F>(&self, id: String, operation: F) -> Result<SessionView, String>
    where
        F: FnOnce(
            &mut Session,
            ClockAnchor,
            ClockReading,
        ) -> Result<Vec<SessionEvent>, DomainError>,
    {
        let mut connection = self.connection.lock().map_err(|e| e.to_string())?;
        let mut clock = self.clock.lock().map_err(|e| e.to_string())?;
        let reading = Self::reading(&clock)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let mut session = load_session(&tx, &id)?.ok_or("Session not found.")?;
        let before = session.clone();
        let anchor = Self::anchor_for(&mut clock, &session, reading);
        let reconciliation = session.reconcile(anchor, reading).map_err(domain_error)?;
        match operation(&mut session, anchor, reading) {
            Ok(events) => {
                let mut all = reconciliation;
                all.extend(events);
                if session != before {
                    persist_session(&tx, &session)?;
                }
                persist_events(&tx, &session.id, &all)?;
                tx.commit().map_err(|e| e.to_string())?;
                Self::update_anchor(&mut clock, &session, reading);
                view(&session, reading, anchor)
            }
            Err(error) => {
                if session != before {
                    persist_session(&tx, &session)?;
                    persist_events(&tx, &session.id, &reconciliation)?;
                    tx.commit().map_err(|e| e.to_string())?;
                    Self::update_anchor(&mut clock, &session, reading);
                }
                Err(domain_error(error))
            }
        }
    }
}

fn active_anchor(clock: &RuntimeClock, session: &Session, reading: ClockReading) -> ClockAnchor {
    clock
        .active_anchor
        .as_ref()
        .filter(|(id, _)| id == &session.id)
        .map(|(_, anchor)| *anchor)
        .unwrap_or(ClockAnchor {
            wall_utc_ms: reading.wall_utc_ms,
            monotonic_ms: reading.monotonic_ms,
        })
}

fn view(
    session: &Session,
    reading: ClockReading,
    anchor: ClockAnchor,
) -> Result<SessionView, String> {
    let (now, _) = Session::effective_now_ms(anchor, reading).map_err(domain_error)?;
    let snapshot = serde_json::from_str(&session.snapshot_json).map_err(|e| e.to_string())?;
    Ok(SessionView {
        id: session.id.clone(),
        profile_id: session.profile_id.clone(),
        profile_version: session.profile_version,
        snapshot,
        planned_duration_minutes: session.planned_duration_minutes,
        started_at_ms: session.started_at_ms,
        deadline_at_ms: session.deadline_at_ms,
        state: session.state,
        completion_policy: session.completion_policy,
        completion_outcome: session.completion_outcome,
        break_index: session.break_index,
        break_deadline_at_ms: session.break_deadline_at_ms,
        remaining_ms: session.remaining_ms(now),
        break_remaining_ms: session
            .break_deadline_at_ms
            .map(|d| d.saturating_sub(now).max(0))
            .unwrap_or(0),
        revision: session.revision,
    })
}

fn load_session(connection: &Connection, id: &str) -> Result<Option<Session>, String> {
    connection.query_row("SELECT id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,completion_outcome,break_index,break_deadline_at_ms,expired_at_ms,revision,clock_anomaly_delta_ms FROM sessions WHERE id=?", [id], session_from_row).optional().map_err(|e|e.to_string())
}
fn load_active(connection: &Connection) -> Result<Option<Session>, String> {
    connection.query_row("SELECT id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,completion_outcome,break_index,break_deadline_at_ms,expired_at_ms,revision,clock_anomaly_delta_ms FROM sessions WHERE state IN ('active_work','active_break') ORDER BY started_at_ms LIMIT 1", [], session_from_row).optional().map_err(|e|e.to_string())
}
fn load_pending(connection: &Connection) -> Result<Vec<Session>, String> {
    let mut stmt = connection.prepare("SELECT id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,completion_outcome,break_index,break_deadline_at_ms,expired_at_ms,revision,clock_anomaly_delta_ms FROM sessions WHERE state='awaiting_completion' ORDER BY started_at_ms DESC").map_err(|e|e.to_string())?;
    stmt.query_map([], session_from_row)
        .map_err(|e| e.to_string())?
        .map(|row| row.map_err(|e| e.to_string()))
        .collect()
}
fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let state: String = row.get(7)?;
    let policy: String = row.get(8)?;
    let outcome: Option<String> = row.get(9)?;
    Ok(Session {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        profile_version: row.get(2)?,
        snapshot_json: row.get(3)?,
        planned_duration_minutes: row.get(4)?,
        started_at_ms: row.get(5)?,
        deadline_at_ms: row.get(6)?,
        state: parse_enum(&state)?,
        completion_policy: parse_enum(&policy)?,
        completion_outcome: outcome.as_deref().map(parse_enum).transpose()?,
        break_index: row.get(10)?,
        break_deadline_at_ms: row.get(11)?,
        expired_at_ms: row.get(12)?,
        revision: u64::try_from(row.get::<_, i64>(13)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        clock_anomaly_delta_ms: row.get(14)?,
    })
}
fn parse_enum<T: for<'de> serde::Deserialize<'de>>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(&format!("\"{value}\"")).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn insert_session(tx: &Transaction<'_>, session: &Session) -> Result<(), String> {
    tx.execute("INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,completion_outcome,break_index,break_deadline_at_ms,expired_at_ms,revision,clock_anomaly_delta_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", params![session.id,session.profile_id,session.profile_version,session.snapshot_json,session.planned_duration_minutes,session.started_at_ms,session.deadline_at_ms,enum_text(session.state)?,enum_text(session.completion_policy)?,session.completion_outcome.map(enum_text).transpose()?,session.break_index,session.break_deadline_at_ms,session.expired_at_ms,i64::try_from(session.revision).map_err(|_| "Session revision overflow.")?,session.clock_anomaly_delta_ms]).map_err(|e|e.to_string())?;
    Ok(())
}
fn persist_session(tx: &Transaction<'_>, session: &Session) -> Result<(), String> {
    let changed = tx.execute("UPDATE sessions SET deadline_at_ms=?,state=?,completion_policy=?,completion_outcome=?,break_index=?,break_deadline_at_ms=?,expired_at_ms=?,revision=?,clock_anomaly_delta_ms=? WHERE id=?", params![session.deadline_at_ms,enum_text(session.state)?,enum_text(session.completion_policy)?,session.completion_outcome.map(enum_text).transpose()?,session.break_index,session.break_deadline_at_ms,session.expired_at_ms,i64::try_from(session.revision).map_err(|_| "Session revision overflow.")?,session.clock_anomaly_delta_ms,session.id]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err("Session state could not be persisted.".into());
    }
    Ok(())
}
fn persist_events(
    tx: &Transaction<'_>,
    session_id: &str,
    events: &[SessionEvent],
) -> Result<(), String> {
    for event in events {
        tx.execute("INSERT INTO session_events(session_id,kind,occurred_at_ms,revision,detail_json) VALUES(?,?,?,?,?)", params![session_id,enum_text(event.kind.clone())?,event.occurred_at_ms,i64::try_from(event.revision).map_err(|_| "Session revision overflow.")?,event.detail_json]).map_err(|e|e.to_string())?;
    }
    Ok(())
}
fn enum_text<T: Serialize>(value: T) -> Result<String, String> {
    serde_json::to_value(value)
        .map_err(|e| e.to_string())?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Invalid enum value.".into())
}
fn domain_error(error: DomainError) -> String {
    format!("Session request rejected: {error:?}.")
}
