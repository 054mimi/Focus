use crate::policy_simulation::{
    LifecycleEvent, LifecycleState, PolicyOperation, PolicyPlan, SessionPolicySnapshot,
    SimulationError, SimulationFault, transition_allowed, validate_operation_id,
    validate_request_id,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn utc_now_ms() -> Result<i64, String> {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_millis();
    i64::try_from(n).map_err(err)
}

pub(crate) fn create(
    connection: &mut Connection,
    session_id: &str,
    expected_version: i64,
    expected_revision: u64,
    request_id: &str,
    now: i64,
) -> Result<PolicyOperation, String> {
    if session_id.is_empty() || session_id.len() > 128 || session_id.contains('\0') {
        return Err("Simulation session ID is invalid.".into());
    }
    validate_request_id(request_id).map_err(domain_error)?;
    let payload = serde_json::json!({
        "action":"prepare", "sessionId":session_id,
        "profileVersion":expected_version, "expectedSessionRevision":expected_revision
    });
    let payload_json = serde_json::to_string(&payload).map_err(err)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    if let Some(op) = find_request(&tx, request_id, "prepare", &payload_json)? {
        tx.commit().map_err(err)?;
        return Ok(op);
    }
    let existing: Option<String> = tx
        .query_row(
            "SELECT operation_id FROM simulated_policy_operations WHERE request_key=?",
            [request_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = existing {
        let current = load(&tx, &id)?;
        if current.plan.session_id != session_id
            || current.plan.profile_version != expected_version
            || current.plan.session_revision != expected_revision
        {
            return Err("Simulation request key is already used for a different request.".into());
        }
        let op = initial_response(&tx, &current)?;
        store_request(&tx, request_id, &id, "prepare", &payload_json, &op, now)?;
        tx.commit().map_err(err)?;
        return Ok(op);
    }
    let source: SessionPolicySnapshot = tx
        .query_row(
            "SELECT profile_version,revision,state,snapshot_json FROM sessions WHERE id=?",
            [session_id],
            |r| {
                Ok(SessionPolicySnapshot {
                    session_id: session_id.to_owned(),
                    profile_version: r.get(0)?,
                    revision: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                    state: r.get(2)?,
                    snapshot_json: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(err)?
        .ok_or("Session does not exist.")?;
    let plan = PolicyPlan::validate(
        &source,
        expected_version,
        expected_revision,
        &source.snapshot_json,
    )
    .map_err(domain_error)?;
    let prior = load_resources(&tx, session_id)?;
    let id_suffix: String = tx
        .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(err)?;
    let mut op = PolicyOperation::new(
        format!("sim-{id_suffix}"),
        request_id.to_owned(),
        plan,
        prior,
    )
    .map_err(domain_error)?;
    op.events[0].occurred_at_ms = now.max(0);
    tx.execute("INSERT INTO simulated_policy_operations(operation_id,request_key,session_id,profile_version,session_revision,snapshot_json,plan_json,prior_resources_json,state,revision,error,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",params![op.operation_id,op.request_key,session_id,op.plan.profile_version,i64::try_from(op.plan.session_revision).map_err(err)?,op.plan.snapshot_json,serde_json::to_string(&op.plan).map_err(err)?,serde_json::to_string(&op.prior_resources).map_err(err)?,op.state.as_str(),i64::try_from(op.revision).map_err(err)?,Option::<String>::None,now.max(0),now.max(0)]).map_err(err)?;
    save_events(&tx, &op.operation_id, &op.events)?;
    store_request(
        &tx,
        request_id,
        &op.operation_id,
        "prepare",
        &payload_json,
        &op,
        now,
    )?;
    tx.commit().map_err(err)?;
    Ok(op)
}

pub(crate) fn get(connection: &Connection, operation_id: &str) -> Result<PolicyOperation, String> {
    validate_operation_id(operation_id).map_err(domain_error)?;
    load(connection, operation_id)
}

pub(crate) fn backfill_requests(connection: &mut Connection, now: i64) -> Result<(), String> {
    let operations = {
        let mut stmt = connection.prepare(
            "SELECT operation_id FROM simulated_policy_operations ORDER BY created_at_ms,operation_id",
        ).map_err(err)?;
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?
    };
    for operation_id in operations {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let operation = load(&tx, &operation_id)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM simulated_policy_requests WHERE request_id=?)",
                [&operation.request_key],
                |row| row.get(0),
            )
            .map_err(err)?;
        if !exists {
            let initial = initial_response(&tx, &operation)?;
            let payload = serde_json::json!({
                "action":"prepare", "sessionId":initial.plan.session_id,
                "profileVersion":initial.plan.profile_version,
                "expectedSessionRevision":initial.plan.session_revision
            });
            let payload_json = serde_json::to_string(&payload).map_err(err)?;
            store_request(
                &tx,
                &operation.request_key,
                &operation.operation_id,
                "prepare",
                &payload_json,
                &initial,
                now,
            )?;
        }
        tx.commit().map_err(err)?;
    }
    Ok(())
}

pub(crate) fn apply(
    connection: &mut Connection,
    operation_id: &str,
    request_id: &str,
    expected_operation_revision: u64,
    expected_session_revision: u64,
    now: i64,
) -> Result<PolicyOperation, String> {
    transition_request(
        connection,
        operation_id,
        request_id,
        "apply",
        expected_operation_revision,
        Some(expected_session_revision),
        now,
        |op| op.apply(SimulationFault::None, now),
    )
}
pub(crate) fn verify(
    connection: &mut Connection,
    operation_id: &str,
    request_id: &str,
    expected_operation_revision: u64,
    expected_session_revision: u64,
    now: i64,
) -> Result<PolicyOperation, String> {
    transition_request(
        connection,
        operation_id,
        request_id,
        "verify",
        expected_operation_revision,
        Some(expected_session_revision),
        now,
        |op| op.verify(SimulationFault::None, now),
    )
}
pub(crate) fn rollback(
    connection: &mut Connection,
    operation_id: &str,
    request_id: &str,
    expected_operation_revision: u64,
    now: i64,
) -> Result<PolicyOperation, String> {
    transition_request(
        connection,
        operation_id,
        request_id,
        "rollback",
        expected_operation_revision,
        None,
        now,
        |op| op.rollback(SimulationFault::None, now),
    )
}

#[cfg(test)]
pub(crate) fn begin_interrupted_apply(
    connection: &mut Connection,
    operation_id: &str,
    request_id: &str,
    expected_operation_revision: u64,
    expected_session_revision: u64,
    now: i64,
) -> Result<PolicyOperation, String> {
    transition_request(
        connection,
        operation_id,
        request_id,
        "apply",
        expected_operation_revision,
        Some(expected_session_revision),
        now,
        |op| op.begin_apply(now),
    )
}

fn transition_request<F>(
    connection: &mut Connection,
    operation_id: &str,
    request_id: &str,
    action_name: &str,
    expected_operation_revision: u64,
    expected_session_revision: Option<u64>,
    now: i64,
    action: F,
) -> Result<PolicyOperation, String>
where
    F: FnOnce(&mut PolicyOperation) -> Result<(), SimulationError>,
{
    validate_request_id(request_id).map_err(domain_error)?;
    validate_operation_id(operation_id).map_err(domain_error)?;
    let payload = serde_json::json!({
        "action":action_name, "operationId":operation_id,
        "expectedOperationRevision":expected_operation_revision,
        "expectedSessionRevision":expected_session_revision
    });
    let payload_json = serde_json::to_string(&payload).map_err(err)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    if let Some(op) = find_request(&tx, request_id, action_name, &payload_json)? {
        tx.commit().map_err(err)?;
        return Ok(op);
    }
    let mut op = load(&tx, operation_id)?;
    if op.revision != expected_operation_revision {
        return Err("Simulation operation revision is stale.".into());
    }
    if let Some(expected_session_revision) = expected_session_revision {
        validate_live_session(&tx, &op, expected_session_revision)?;
    }
    let old_state = op.state;
    let old_revision = op.revision;
    let old_events = op.events.len();
    action(&mut op).map_err(domain_error)?;
    persist(&tx, &op, old_state, old_revision, old_events, now)?;
    store_request(
        &tx,
        request_id,
        operation_id,
        action_name,
        &payload_json,
        &op,
        now,
    )?;
    tx.commit().map_err(err)?;
    Ok(op)
}
fn persist(
    tx: &Transaction<'_>,
    op: &PolicyOperation,
    old_state: LifecycleState,
    old_revision: u64,
    old_events: usize,
    now: i64,
) -> Result<(), String> {
    let mut persisted_state = old_state;
    let mut persisted_revision = old_revision;
    for (index, event) in op.events.iter().enumerate().skip(old_events) {
        let next_state = event_state(&event.kind)
            .ok_or_else(|| format!("Unknown simulation lifecycle event: {}", event.kind))?;
        if !transition_allowed(persisted_state, next_state)
            || event.revision != persisted_revision.saturating_add(1)
        {
            return Err("Invalid simulated policy transition or revision.".into());
        }
        let final_event = index + 1 == op.events.len();
        let error = if final_event {
            op.error.as_deref()
        } else {
            None
        };
        let changed = tx.execute(
            "UPDATE simulated_policy_operations SET state=?,revision=?,error=?,updated_at_ms=? WHERE operation_id=? AND state=? AND revision=?",
            params![next_state.as_str(), i64::try_from(event.revision).map_err(err)?, error, event.occurred_at_ms.max(0), op.operation_id, persisted_state.as_str(), i64::try_from(persisted_revision).map_err(err)?],
        ).map_err(err)?;
        if changed != 1 {
            return Err("Simulation operation changed concurrently.".into());
        }
        tx.execute(
            "INSERT INTO simulated_policy_events(operation_id,revision,kind,occurred_at_ms,detail) VALUES(?,?,?,?,?)",
            params![op.operation_id, i64::try_from(event.revision).map_err(err)?, event.kind, event.occurred_at_ms.max(0), event.detail],
        ).map_err(err)?;
        persisted_state = next_state;
        persisted_revision = event.revision;
    }
    if persisted_state != op.state || persisted_revision != op.revision {
        return Err("Simulation journal and operation state disagree.".into());
    }
    if op.simulated_resources != op.prior_resources
        || op.state == LifecycleState::Applied
        || op.state == LifecycleState::RollbackPending
        || op.state == LifecycleState::RolledBack
        || op.state == LifecycleState::Failed
    {
        tx.execute(
            "DELETE FROM simulated_policy_resources WHERE session_id=?",
            [&op.plan.session_id],
        )
        .map_err(err)?;
        for (key, value) in &op.simulated_resources {
            tx.execute("INSERT INTO simulated_policy_resources(session_id,resource_key,value_json) VALUES(?,?,?)",params![op.plan.session_id,key,serde_json::to_string(value).map_err(err)?]).map_err(err)?;
        }
    }
    let _ = now;
    Ok(())
}

fn event_state(kind: &str) -> Option<LifecycleState> {
    match kind {
        "planned" => Some(LifecycleState::Planned),
        "applying" => Some(LifecycleState::Applying),
        "applied" => Some(LifecycleState::Applied),
        "verified" => Some(LifecycleState::Verified),
        "partial_apply"
        | "apply_timed_out_partial"
        | "verification_failed"
        | "verification_timed_out"
        | "rollback_pending" => Some(LifecycleState::RollbackPending),
        "apply_failed" | "apply_timed_out" | "rollback_failed" | "rollback_timed_out"
        | "recovery_failed" => Some(LifecycleState::Failed),
        "rolled_back" | "recovered_rollback" => Some(LifecycleState::RolledBack),
        _ => None,
    }
}
fn save_events(
    tx: &Transaction<'_>,
    operation_id: &str,
    events: &[LifecycleEvent],
) -> Result<(), String> {
    for e in events {
        tx.execute("INSERT INTO simulated_policy_events(operation_id,revision,kind,occurred_at_ms,detail) VALUES(?,?,?,?,?)",params![operation_id,i64::try_from(e.revision).map_err(err)?,e.kind,e.occurred_at_ms,e.detail]).map_err(err)?;
    }
    Ok(())
}
fn load(connection: &Connection, operation_id: &str) -> Result<PolicyOperation, String> {
    let row=connection.query_row("SELECT operation_id,request_key,session_id,profile_version,session_revision,snapshot_json,plan_json,prior_resources_json,state,revision,error FROM simulated_policy_operations WHERE operation_id=?",[operation_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,i64>(9)?,r.get::<_,Option<String>>(10)?))).optional().map_err(err)?.ok_or("Simulation operation not found.")?;
    let events = {
        let mut stmt=connection.prepare("SELECT revision,kind,occurred_at_ms,detail FROM simulated_policy_events WHERE operation_id=? ORDER BY revision").map_err(err)?;
        stmt.query_map([operation_id], |r| {
            Ok(LifecycleEvent {
                revision: u64::try_from(r.get::<_, i64>(0)?).unwrap_or(u64::MAX),
                kind: r.get(1)?,
                occurred_at_ms: r.get(2)?,
                detail: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?
    };
    let plan: PolicyPlan = serde_json::from_str(&row.6).map_err(err)?;
    let prior_resources: BTreeMap<String, Value> = serde_json::from_str(&row.7).map_err(err)?;
    let state = match row.8.as_str() {
        "planned" => LifecycleState::Planned,
        "applying" => LifecycleState::Applying,
        "applied" => LifecycleState::Applied,
        "verified" => LifecycleState::Verified,
        "rollback_pending" => LifecycleState::RollbackPending,
        "rolled_back" => LifecycleState::RolledBack,
        "failed" => LifecycleState::Failed,
        _ => return Err("Stored simulation lifecycle state is invalid.".into()),
    };
    let simulated_resources = load_resources(connection, &plan.session_id)?;
    let op = PolicyOperation {
        operation_id: row.0,
        request_key: row.1,
        plan,
        prior_resources,
        simulated_resources,
        state,
        revision: u64::try_from(row.9).map_err(err)?,
        error: row.10,
        events,
    };
    if op.plan.session_id != row.2
        || op.plan.profile_version != row.3
        || i64::try_from(op.plan.session_revision).map_err(err)? != row.4
        || op.plan.snapshot_json != row.5
        || op.events.len() as i64 != row.9
        || op
            .events
            .iter()
            .enumerate()
            .any(|(i, event)| event.revision != (i + 1) as u64)
        || op
            .events
            .last()
            .is_none_or(|event| event_state(&event.kind) != Some(op.state))
    {
        return Err("Stored simulation journal is inconsistent.".into());
    }
    Ok(op)
}
fn load_resources(
    connection: &Connection,
    session_id: &str,
) -> Result<BTreeMap<String, Value>, String> {
    let mut stmt=connection.prepare("SELECT resource_key,value_json FROM simulated_policy_resources WHERE session_id=? ORDER BY resource_key").map_err(err)?;
    stmt.query_map([session_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })
    .map_err(err)?
    .map(|row| {
        let (k, v) = row.map_err(err)?;
        Ok((k, serde_json::from_str(&v).map_err(err)?))
    })
    .collect()
}

fn find_request(
    connection: &Connection,
    request_id: &str,
    action: &str,
    payload_json: &str,
) -> Result<Option<PolicyOperation>, String> {
    let existing = connection.query_row(
        "SELECT operation_id,action,payload_json,result_json FROM simulated_policy_requests WHERE request_id=?",
        [request_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)),
    ).optional().map_err(err)?;
    let Some((operation_id, saved_action, saved_payload, result_json)) = existing else {
        return Ok(None);
    };
    if saved_action != action || saved_payload != payload_json {
        return Err(
            "Simulation request ID is already used for a different action or payload.".into(),
        );
    }
    let result = match result_json {
        Some(json) => serde_json::from_str(&json).map_err(err)?,
        None => load(connection, &operation_id)?,
    };
    Ok(Some(result))
}

fn store_request(
    tx: &Transaction<'_>,
    request_id: &str,
    operation_id: &str,
    action: &str,
    payload_json: &str,
    result: &PolicyOperation,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO simulated_policy_requests(request_id,operation_id,action,payload_json,result_json,created_at_ms) VALUES(?,?,?,?,?,?)",
        params![request_id, operation_id, action, payload_json, serde_json::to_string(result).map_err(err)?, now.max(0)],
    ).map_err(err)?;
    Ok(())
}

fn initial_response(
    _connection: &Connection,
    current: &PolicyOperation,
) -> Result<PolicyOperation, String> {
    let mut initial = PolicyOperation::new(
        current.operation_id.clone(),
        current.request_key.clone(),
        current.plan.clone(),
        current.prior_resources.clone(),
    )
    .map_err(domain_error)?;
    if let Some(event) = current.events.first() {
        initial.events[0].occurred_at_ms = event.occurred_at_ms;
    }
    Ok(initial)
}

fn validate_live_session(
    connection: &Connection,
    operation: &PolicyOperation,
    expected_revision: u64,
) -> Result<(), String> {
    let current: Option<(i64, i64, String, String)> = connection
        .query_row(
            "SELECT profile_version,revision,state,snapshot_json FROM sessions WHERE id=?",
            [&operation.plan.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(err)?;
    let Some((version, revision, state, snapshot)) = current else {
        return Err("Simulation session no longer exists.".into());
    };
    if !matches!(state.as_str(), "active_work" | "active_break")
        || u64::try_from(revision).ok() != Some(expected_revision)
        || u64::try_from(revision).ok() != Some(operation.plan.session_revision)
        || version != operation.plan.profile_version
        || snapshot != operation.plan.snapshot_json
    {
        return Err(
            "Simulation session is inactive or its approved snapshot/revision is stale.".into(),
        );
    }
    Ok(())
}

pub(crate) fn recover_incomplete(connection: &mut Connection, now: i64) -> Result<(), String> {
    let ids = {
        let mut stmt=connection.prepare("SELECT operation_id FROM simulated_policy_operations WHERE state IN ('applying','rollback_pending') ORDER BY created_at_ms,operation_id").map_err(err)?;
        stmt.query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?
    };
    for id in ids {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let mut op = match load(&tx, &id) {
            Ok(op) => op,
            Err(_) => {
                let (revision,) = tx
                    .query_row(
                        "SELECT revision FROM simulated_policy_operations WHERE operation_id=?",
                        [&id],
                        |r| Ok((r.get::<_, i64>(0)?,)),
                    )
                    .map_err(err)?;
                let next = revision.saturating_add(1);
                tx.execute("UPDATE simulated_policy_operations SET state='failed',revision=?,error='invalid_recovery_data',updated_at_ms=? WHERE operation_id=?",params![next,now.max(0),id]).map_err(err)?;
                tx.execute("INSERT INTO simulated_policy_events(operation_id,revision,kind,occurred_at_ms,detail) VALUES(?,?,'recovery_failed',?,'invalid_recovery_data')",params![id,next,now.max(0)]).map_err(err)?;
                tx.commit().map_err(err)?;
                continue;
            }
        };
        let old_state = op.state;
        let old_revision = op.revision;
        let old_events = op.events.len();
        if op.recover(now).is_err() {
            op.state = LifecycleState::Failed;
            op.revision = op.revision.saturating_add(1);
            op.error = Some("invalid_recovery_data".into());
            op.events.push(LifecycleEvent {
                revision: op.revision,
                kind: "recovery_failed".into(),
                occurred_at_ms: now.max(0),
                detail: "invalid_recovery_data".into(),
            });
        }
        persist(&tx, &op, old_state, old_revision, old_events, now)?;
        tx.commit().map_err(err)?;
    }
    Ok(())
}
fn domain_error(e: SimulationError) -> String {
    format!("Simulated policy request rejected: {e:?}.")
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    const SNAPSHOT: &str = r#"{"applicationPolicy":{"mode":"whitelist","entries":["editor.exe"]},"websitePolicy":{"mode":"disabled","domains":[]},"networkPolicy":{"mode":"disabled","applications":[]}}"#;

    fn seeded_database(path: &std::path::Path) {
        let mut connection = Connection::open(path).unwrap();
        crate::storage::migrate(&mut connection).unwrap();
        connection.execute(
            "INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,revision) VALUES('session','profile',1,?,45,1,9999999999999,'active_work','timer_only',0)",
            [SNAPSHOT],
        ).unwrap();
    }

    fn database_path() -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("focusmimi-requests-{stamp}.sqlite"))
    }

    #[test]
    fn concurrent_same_request_id_returns_one_original_operation() {
        let path = database_path();
        seeded_database(&path);
        let gate = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let path = path.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    let mut connection = Connection::open(path).unwrap();
                    crate::storage::configure_connection(&connection).unwrap();
                    gate.wait();
                    create(&mut connection, "session", 1, 0, "same-request", 10)
                        .unwrap()
                        .operation_id
                })
            })
            .collect::<Vec<_>>();
        let ids = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids[0], ids[1]);
        let connection = Connection::open(&path).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_operations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn distinct_concurrent_requests_cannot_create_two_active_operations() {
        let path = database_path();
        seeded_database(&path);
        let gate = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|i| {
                let path = path.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    let mut connection = Connection::open(path).unwrap();
                    crate::storage::configure_connection(&connection).unwrap();
                    gate.wait();
                    create(
                        &mut connection,
                        "session",
                        1,
                        0,
                        &format!("distinct-{i}"),
                        10,
                    )
                    .is_ok()
                })
            })
            .collect::<Vec<_>>();
        let successes = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(successes, 1);
        let connection = Connection::open(&path).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_operations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn concurrent_actions_with_same_revision_allow_only_one_transition() {
        let path = database_path();
        seeded_database(&path);
        let mut connection = Connection::open(&path).unwrap();
        crate::storage::configure_connection(&connection).unwrap();
        let operation = create(&mut connection, "session", 1, 0, "action-race-prepare", 10).unwrap();
        drop(connection);
        let gate = Arc::new(Barrier::new(2));
        let handles = (0..2).map(|i| {
            let path = path.clone();
            let gate = gate.clone();
            let operation_id = operation.operation_id.clone();
            std::thread::spawn(move || {
                let mut connection = Connection::open(path).unwrap();
                crate::storage::configure_connection(&connection).unwrap();
                gate.wait();
                apply(&mut connection, &operation_id, &format!("racing-apply-{i}"), 1, 0, 20).is_ok()
            })
        }).collect::<Vec<_>>();
        let successes = handles.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count();
        assert_eq!(successes, 1);
        let connection = Connection::open(&path).unwrap();
        let op = load(&connection, &operation.operation_id).unwrap();
        assert_eq!(op.state, LifecycleState::Applied);
        assert_eq!(op.revision, 3);
        assert_eq!(op.events.len(), 3);
        let apply_requests: i64 = connection.query_row("SELECT COUNT(*) FROM simulated_policy_requests WHERE action='apply'", [], |r| r.get(0)).unwrap();
        assert_eq!(apply_requests, 1);
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn event_persistence_failure_rolls_back_operation_and_request_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::storage::migrate(&mut connection).unwrap();
        connection.execute(
            "INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,revision) VALUES('session','profile',1,?,45,1,9999999999999,'active_work','timer_only',0)",
            [SNAPSHOT],
        ).unwrap();
        connection.execute_batch("CREATE TEMP TRIGGER fail_simulation_event BEFORE INSERT ON simulated_policy_events BEGIN SELECT RAISE(ABORT, 'injected event persistence failure'); END;").unwrap();
        assert!(create(&mut connection, "session", 1, 0, "persist-failure", 10).is_err());
        let operations: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_operations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let requests: i64 = connection
            .query_row("SELECT COUNT(*) FROM simulated_policy_requests", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!((operations, requests), (0, 0));
        connection
            .execute_batch("DROP TRIGGER fail_simulation_event;")
            .unwrap();
        let operation = create(&mut connection, "session", 1, 0, "persist-retry", 11).unwrap();
        connection.execute_batch("CREATE TEMP TRIGGER fail_simulation_event BEFORE INSERT ON simulated_policy_events BEGIN SELECT RAISE(ABORT, 'injected event persistence failure'); END;").unwrap();
        assert!(
            apply(
                &mut connection,
                &operation.operation_id,
                "apply-persist-failure",
                operation.revision,
                0,
                12
            )
            .is_err()
        );
        let state: (String, i64) = connection
            .query_row(
                "SELECT state,revision FROM simulated_policy_operations WHERE operation_id=?",
                [&operation.operation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let resources: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_resources WHERE session_id='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let apply_requests: i64 = connection.query_row("SELECT COUNT(*) FROM simulated_policy_requests WHERE request_id='apply-persist-failure'", [], |r| r.get(0)).unwrap();
        assert_eq!(state, ("planned".into(), 1));
        assert_eq!((resources, apply_requests), (0, 0));
    }

    #[test]
    fn apply_rejects_stale_or_inactive_session_without_fake_resource_changes() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::storage::migrate(&mut connection).unwrap();
        connection.execute(
            "INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy,revision) VALUES('session','profile',1,?,45,1,9999999999999,'active_work','timer_only',0)",
            [SNAPSHOT],
        ).unwrap();
        let operation = create(&mut connection, "session", 1, 0, "stale-session", 10).unwrap();
        connection
            .execute("UPDATE sessions SET revision=1 WHERE id='session'", [])
            .unwrap();
        assert!(
            apply(
                &mut connection,
                &operation.operation_id,
                "stale-apply",
                operation.revision,
                0,
                11
            )
            .is_err()
        );
        connection
            .execute(
                "UPDATE sessions SET revision=0,state='completed' WHERE id='session'",
                [],
            )
            .unwrap();
        assert!(
            apply(
                &mut connection,
                &operation.operation_id,
                "inactive-apply",
                operation.revision,
                0,
                12
            )
            .is_err()
        );
        let resources: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_resources WHERE session_id='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(resources, 0);
    }
}
