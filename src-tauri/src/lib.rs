mod policy_simulation;
mod policy_simulation_store;
mod session_domain;
mod session_runtime;
mod storage;

use policy_simulation::PolicyOperation;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use session_domain::CompletionOutcome;
use session_runtime::{AppDatabase, SessionStatus, SessionView};
use tauri::Manager;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileRecord {
    id: String,
    name: String,
    description: String,
    visible: bool,
    sort_order: i64,
    approved_version: Option<i64>,
    versions: Vec<VersionRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionRecord {
    version: i64,
    status: String,
    config: serde_json::Value,
    created_at: String,
    approved_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NamedRecord {
    id: String,
    name: String,
    description: Option<String>,
    project_id: Option<String>,
    completed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileInput {
    id: Option<String>,
    name: String,
    description: String,
    config: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedInput {
    id: Option<String>,
    name: String,
    description: Option<String>,
    project_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum PolicySimulationRequest {
    Prepare {
        session_id: String,
        expected_profile_version: i64,
        expected_revision: u64,
        request_id: String,
    },
    Apply {
        operation_id: String,
        request_id: String,
        expected_operation_revision: u64,
        expected_session_revision: u64,
    },
    Verify {
        operation_id: String,
        request_id: String,
        expected_operation_revision: u64,
        expected_session_revision: u64,
    },
    Rollback {
        operation_id: String,
        request_id: String,
        expected_operation_revision: u64,
    },
    Status {
        operation_id: String,
    },
}

#[tauri::command]
fn simulate_session_policy(
    request: PolicySimulationRequest,
    db: tauri::State<'_, AppDatabase>,
) -> Result<PolicyOperation, String> {
    simulate_session_policy_inner(request, &db)
}

fn simulate_session_policy_inner(
    request: PolicySimulationRequest,
    db: &AppDatabase,
) -> Result<PolicyOperation, String> {
    match request {
        PolicySimulationRequest::Prepare {
            session_id,
            expected_profile_version,
            expected_revision,
            request_id,
        } => db.create_policy_simulation(
            &session_id,
            expected_profile_version,
            expected_revision,
            &request_id,
        ),
        PolicySimulationRequest::Apply {
            operation_id,
            request_id,
            expected_operation_revision,
            expected_session_revision,
        } => db.apply_simulated_policy(
            &operation_id,
            &request_id,
            expected_operation_revision,
            expected_session_revision,
        ),
        PolicySimulationRequest::Verify {
            operation_id,
            request_id,
            expected_operation_revision,
            expected_session_revision,
        } => db.verify_simulated_policy(
            &operation_id,
            &request_id,
            expected_operation_revision,
            expected_session_revision,
        ),
        PolicySimulationRequest::Rollback {
            operation_id,
            request_id,
            expected_operation_revision,
        } => db.rollback_simulated_policy(&operation_id, &request_id, expected_operation_revision),
        PolicySimulationRequest::Status { operation_id } => db.get_policy_simulation(&operation_id),
    }
}

fn profile_list(db: &AppDatabase) -> Result<Vec<ProfileRecord>, String> {
    let connection = db.connection.lock().map_err(|e| e.to_string())?;
    let mut stmt = connection.prepare("SELECT id,name,description,visible,sort_order,approved_version FROM profiles ORDER BY sort_order,name") .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (id, name, description, visible, sort_order, approved_version) =
            row.map_err(|e| e.to_string())?;
        let mut versions_stmt = connection.prepare("SELECT version,status,config_json,created_at,approved_at FROM profile_versions WHERE profile_id=? ORDER BY version DESC").map_err(|e|e.to_string())?;
        let versions = versions_stmt
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .map(|v| {
                let (version, status, json, created_at, approved_at) =
                    v.map_err(|e| e.to_string())?;
                Ok(VersionRecord {
                    version,
                    status,
                    config: serde_json::from_str(&json).map_err(|e| e.to_string())?,
                    created_at,
                    approved_at,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        result.push(ProfileRecord {
            id,
            name,
            description,
            visible,
            sort_order,
            approved_version,
            versions,
        });
    }
    Ok(result)
}

fn validate_profile_config(config: &serde_json::Value) -> Result<(), String> {
    let activity = config
        .get("activity")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let outcome = config
        .get("intendedOutcome")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let duration = config
        .get("durationMinutes")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if activity.trim().is_empty() || activity.len() > 80 {
        return Err("Activity must be 1–80 characters.".into());
    }
    if outcome.trim().is_empty() || outcome.len() > 500 {
        return Err("Intended outcome must be 1–500 characters.".into());
    }
    if !(5..=1440).contains(&duration) {
        return Err("Duration must be between 5 minutes and 24 hours.".into());
    }
    for key in ["applicationPolicy", "websitePolicy", "networkPolicy"] {
        let policy = config.get(key).ok_or_else(|| format!("Missing {key}."))?;
        let mode = policy.get("mode").and_then(|v| v.as_str()).unwrap_or("");
        if !["disabled", "whitelist", "blacklist"].contains(&mode) {
            return Err(format!("Invalid {key} mode."));
        }
        let values = policy
            .get("entries")
            .or_else(|| policy.get("domains"))
            .or_else(|| policy.get("applications"));
        let values = values
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Invalid {key} entries."))?;
        if mode != "disabled"
            && !values
                .iter()
                .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        {
            return Err(format!("Choose at least one item for {key}."));
        }
        if values
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 512 || s.contains('\0')))
        {
            return Err(format!("Invalid {key} entry."));
        }
    }
    let breaks = config
        .get("plannedBreaks")
        .and_then(|v| v.as_array())
        .ok_or("Invalid planned breaks.")?;
    if breaks.len() > 8
        || breaks.iter().any(|v| {
            !v.get("durationMinutes")
                .and_then(|n| n.as_i64())
                .is_some_and(|n| (1..=180).contains(&n))
        })
    {
        return Err("Add at most 8 breaks, each 1–180 minutes.".into());
    }
    Ok(())
}

#[tauri::command]
fn list_profiles(db: tauri::State<'_, AppDatabase>) -> Result<Vec<ProfileRecord>, String> {
    profile_list(&db)
}

#[tauri::command]
fn save_profile_proposal(
    input: ProfileInput,
    db: tauri::State<'_, AppDatabase>,
) -> Result<(), String> {
    if input.name.trim().is_empty()
        || input.name.trim().len() > 80
        || input.description.len() > 1000
    {
        return Err("Profile name or description is outside the allowed length.".into());
    }
    if !input.config.is_object() {
        return Err("Profile configuration must be an object.".into());
    }
    validate_profile_config(&input.config)?;
    let mut config_value = input.config;
    let config_object = config_value
        .as_object_mut()
        .ok_or("Profile configuration must be an object.")?;
    config_object.insert(
        "name".into(),
        serde_json::Value::String(input.name.trim().to_owned()),
    );
    config_object.insert(
        "description".into(),
        serde_json::Value::String(input.description.trim().to_owned()),
    );
    let config = config_value.to_string();
    let mut connection = db.connection.lock().map_err(|e| e.to_string())?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    let (id, generated_id) = match input.id {
        Some(id) => (id, false),
        None => (
            format!(
                "profile-{}",
                tx.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r
                    .get::<_, String>(0))
                    .map_err(|e| e.to_string())?
            ),
            true,
        ),
    };
    if generated_id {
        tx.execute(
            "INSERT INTO profiles(id,name,description) VALUES(?,?,?)",
            params![id, input.name.trim(), input.description.trim()],
        )
        .map_err(|e| e.to_string())?;
    } else {
        tx.execute("INSERT INTO profiles(id,name,description) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')", params![id,input.name.trim(),input.description.trim()]).map_err(|e|e.to_string())?;
    }
    tx.execute(
        "UPDATE profile_versions SET status='superseded' WHERE profile_id=? AND status='proposed'",
        [&id],
    )
    .map_err(|e| e.to_string())?;
    let version: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM profile_versions WHERE profile_id=?",
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO profile_versions(profile_id,version,status,config_json) VALUES(?,?,'proposed',?)", params![id,version,config]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
fn approve_profile_version(
    profile_id: String,
    version: i64,
    db: tauri::State<'_, AppDatabase>,
) -> Result<(), String> {
    let mut connection = db.connection.lock().map_err(|e| e.to_string())?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    let changed = tx.execute("UPDATE profile_versions SET status='approved',approved_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE profile_id=? AND version=? AND status='proposed'", params![profile_id,version]).map_err(|e|e.to_string())?;
    if changed == 0 {
        return Err("Only a proposed version can be approved.".into());
    }
    tx.execute("UPDATE profiles SET approved_version=?,name=json_extract((SELECT config_json FROM profile_versions WHERE profile_id=? AND version=?),'$.name'),description=json_extract((SELECT config_json FROM profile_versions WHERE profile_id=? AND version=?),'$.description'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?", params![version,profile_id,version,profile_id,version,profile_id]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_profile(profile_id: String, db: tauri::State<'_, AppDatabase>) -> Result<(), String> {
    let connection = db.connection.lock().map_err(|e| e.to_string())?;
    connection
        .execute("DELETE FROM profiles WHERE id=?", [&profile_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn set_profile_visible(
    profile_id: String,
    visible: bool,
    db: tauri::State<'_, AppDatabase>,
) -> Result<(), String> {
    let connection = db.connection.lock().map_err(|e| e.to_string())?;
    connection.execute("UPDATE profiles SET visible=?,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?", params![visible, profile_id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn set_profile_order(
    profile_ids: Vec<String>,
    db: tauri::State<'_, AppDatabase>,
) -> Result<(), String> {
    let mut connection = db.connection.lock().map_err(|e| e.to_string())?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    let count: i64 = tx
        .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if count as usize != profile_ids.len()
        || profile_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != profile_ids.len()
    {
        return Err("Profile order must include every saved profile exactly once.".into());
    }
    for (position, id) in profile_ids.iter().enumerate() {
        let changed = tx.execute("UPDATE profiles SET sort_order=?,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?", params![position as i64, id]).map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("Profile order contains an unknown profile.".into());
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_projects(db: tauri::State<'_, AppDatabase>) -> Result<Vec<NamedRecord>, String> {
    list_named(&db, "projects")
}
#[tauri::command]
fn list_tasks(db: tauri::State<'_, AppDatabase>) -> Result<Vec<NamedRecord>, String> {
    let c = db.connection.lock().map_err(|e| e.to_string())?;
    let mut s = c
        .prepare("SELECT id,name,project_id,completed FROM tasks ORDER BY completed,name")
        .map_err(|e| e.to_string())?;
    s.query_map([], |r| {
        Ok(NamedRecord {
            id: r.get(0)?,
            name: r.get(1)?,
            description: None,
            project_id: r.get(2)?,
            completed: Some(r.get(3)?),
        })
    })
    .map_err(|e| e.to_string())?
    .map(|x| x.map_err(|e| e.to_string()))
    .collect()
}
fn list_named(db: &AppDatabase, table: &str) -> Result<Vec<NamedRecord>, String> {
    let c = db.connection.lock().map_err(|e| e.to_string())?;
    let sql = format!("SELECT id,name,description FROM {} ORDER BY name", table);
    let mut s = c.prepare(&sql).map_err(|e| e.to_string())?;
    s.query_map([], |r| {
        Ok(NamedRecord {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            project_id: None,
            completed: None,
        })
    })
    .map_err(|e| e.to_string())?
    .map(|x| x.map_err(|e| e.to_string()))
    .collect()
}
#[tauri::command]
fn save_project(input: NamedInput, db: tauri::State<'_, AppDatabase>) -> Result<(), String> {
    save_named(input, &db, true)
}
#[tauri::command]
fn save_task(input: NamedInput, db: tauri::State<'_, AppDatabase>) -> Result<(), String> {
    save_named(input, &db, false)
}
fn save_named(input: NamedInput, db: &AppDatabase, project: bool) -> Result<(), String> {
    if input.name.trim().is_empty() || input.name.len() > 160 {
        return Err("Name must be between 1 and 160 characters.".into());
    }
    let c = db.connection.lock().map_err(|e| e.to_string())?;
    let (id, generated_id) = match input.id {
        Some(id) => (id, false),
        None => (
            format!(
                "item-{}",
                c.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r
                    .get::<_, String>(0))
                    .map_err(|e| e.to_string())?
            ),
            true,
        ),
    };
    if project {
        let sql = if generated_id {
            "INSERT INTO projects(id,name,description) VALUES(?,?,?)"
        } else {
            "INSERT INTO projects(id,name,description) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')"
        };
        c.execute(
            sql,
            params![id, input.name.trim(), input.description.unwrap_or_default()],
        )
        .map_err(|e| e.to_string())?;
    } else {
        let sql = if generated_id {
            "INSERT INTO tasks(id,name,project_id) VALUES(?,?,?)"
        } else {
            "INSERT INTO tasks(id,name,project_id) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,project_id=excluded.project_id,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')"
        };
        c.execute(sql, params![id, input.name.trim(), input.project_id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_session_status(db: tauri::State<'_, AppDatabase>) -> Result<SessionStatus, String> {
    db.status()
}
#[tauri::command]
fn start_session(
    profile_id: String,
    profile_version: i64,
    confirmed: bool,
    db: tauri::State<'_, AppDatabase>,
) -> Result<SessionView, String> {
    db.start(profile_id, profile_version, confirmed)
}
#[tauri::command]
fn shorten_session_time(
    session_id: String,
    expected_revision: u64,
    remaining_minutes: u32,
    db: tauri::State<'_, AppDatabase>,
) -> Result<SessionView, String> {
    db.shorten(session_id, expected_revision, remaining_minutes)
}
#[tauri::command]
fn start_session_break(
    session_id: String,
    expected_revision: u64,
    db: tauri::State<'_, AppDatabase>,
) -> Result<SessionView, String> {
    db.start_break(session_id, expected_revision)
}
#[tauri::command]
fn skip_session_break(
    session_id: String,
    expected_revision: u64,
    db: tauri::State<'_, AppDatabase>,
) -> Result<SessionView, String> {
    db.skip_break(session_id, expected_revision)
}
#[tauri::command]
fn record_session_outcome(
    session_id: String,
    expected_revision: u64,
    outcome: CompletionOutcome,
    db: tauri::State<'_, AppDatabase>,
) -> Result<SessionView, String> {
    db.record_outcome(session_id, expected_revision, outcome)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("focusmimi.sqlite3");
            app.manage(AppDatabase::open(&db_path)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            save_profile_proposal,
            approve_profile_version,
            delete_profile,
            set_profile_visible,
            set_profile_order,
            list_projects,
            list_tasks,
            save_project,
            save_task,
            get_session_status,
            start_session,
            shorten_session_time,
            start_session_break,
            skip_session_break,
            record_session_outcome,
            simulate_session_policy
        ])
        .run(tauri::generate_context!())
        .expect("failed to run FOCUS by MIMI");
}

#[cfg(test)]
mod tests {
    use crate::storage;
    use rusqlite::Connection;
    #[test]
    fn migrations_upgrade_to_workspace_schema_and_are_idempotent() {
        let mut c = Connection::open_in_memory().unwrap();
        storage::migrate(&mut c).unwrap();
        storage::migrate(&mut c).unwrap();
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 5);
        let count:i64=c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('profiles','profile_versions','projects','tasks','sessions','session_events','simulated_policy_operations','simulated_policy_resources','simulated_policy_events','simulated_policy_requests')",[],|r|r.get(0)).unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn version_one_user_data_survives_workspace_upgrade() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        c.execute("INSERT INTO profiles(id,name,description) VALUES('existing','Existing profile','Keep me')", []).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        storage::migrate(&mut c).unwrap();
        let row: (String, String) = c
            .query_row(
                "SELECT name,description FROM profiles WHERE id='existing'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("Existing profile".into(), "Keep me".into()));
    }

    #[test]
    fn profile_policy_validation_rejects_invalid_and_accepts_supported_shape() {
        let valid = serde_json::json!({
            "activity":"Revise", "intendedOutcome":"Complete notes", "durationMinutes":45,
            "applicationPolicy":{"mode":"disabled","entries":[]},
            "websitePolicy":{"mode":"disabled","domains":[]},
            "networkPolicy":{"mode":"disabled","applications":[]},
            "plannedBreaks":[]
        });
        assert!(super::validate_profile_config(&valid).is_ok());
        let mut invalid = valid;
        invalid["durationMinutes"] = serde_json::json!(0);
        assert!(super::validate_profile_config(&invalid).is_err());
    }

    #[test]
    fn profile_versions_preserve_approved_snapshot_until_explicit_approval() {
        let mut c = Connection::open_in_memory().unwrap();
        storage::migrate(&mut c).unwrap();
        c.execute("INSERT INTO profiles(id,name,description,approved_version) VALUES('p','Old name','Old description',1)", []).unwrap();
        c.execute("INSERT INTO profile_versions(profile_id,version,status,config_json,approved_at) VALUES('p',1,'approved','{\"name\":\"Old name\",\"intendedOutcome\":\"Old outcome\"}','now')", []).unwrap();
        c.execute("INSERT INTO profile_versions(profile_id,version,status,config_json) VALUES('p',2,'proposed','{\"name\":\"New name\",\"intendedOutcome\":\"New outcome\"}')", []).unwrap();
        let (approved_version, name, old_snapshot): (i64, String, String) = c.query_row("SELECT p.approved_version,p.name,v.config_json FROM profiles p JOIN profile_versions v ON v.profile_id=p.id AND v.version=1 WHERE p.id='p'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!((approved_version, name), (1, "Old name".into()));
        assert!(old_snapshot.contains("Old outcome"));
        c.execute("UPDATE profile_versions SET status='approved',approved_at='now' WHERE profile_id='p' AND version=2 AND status='proposed'", []).unwrap();
        c.execute("UPDATE profiles SET approved_version=2,name=json_extract((SELECT config_json FROM profile_versions WHERE profile_id='p' AND version=2),'$.name') WHERE id='p'", []).unwrap();
        let (current, name, old_status, new_status): (i64, String, String, String) = c.query_row("SELECT p.approved_version,p.name,old.status,new.status FROM profiles p JOIN profile_versions old ON old.profile_id=p.id AND old.version=1 JOIN profile_versions new ON new.profile_id=p.id AND new.version=2 WHERE p.id='p'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(
            (current, name, old_status, new_status),
            (2, "New name".into(), "approved".into(), "approved".into())
        );
    }

    #[test]
    fn migration_failure_rolls_back_version_and_keeps_existing_data() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        c.execute(
            "INSERT INTO profiles(id,name,description) VALUES('kept','Kept','data')",
            [],
        )
        .unwrap();
        c.execute_batch(include_str!("../migrations/0002_workspace.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        c.execute("CREATE TABLE sessions(id TEXT)", []).unwrap();
        assert!(storage::migrate(&mut c).is_err());
        let version: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let name: String = c
            .query_row("SELECT name FROM profiles WHERE id='kept'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Kept");
        let events: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(events, 0);
    }

    #[test]
    fn simulated_policy_migration_failure_preserves_v3_schema_and_data() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute_batch(include_str!("../migrations/0002_workspace.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        c.execute_batch(include_str!("../migrations/0003_sessions.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 3).unwrap();
        c.execute(
            "INSERT INTO profiles(id,name,description) VALUES('kept','Kept','data')",
            [],
        )
        .unwrap();
        c.pragma_update(None, "user_version", 3).unwrap();
        c.execute(
            "CREATE TABLE simulated_policy_operations(operation_id TEXT)",
            [],
        )
        .unwrap();
        assert!(storage::migrate(&mut c).is_err());
        let version: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let name: String = c
            .query_row("SELECT name FROM profiles WHERE id='kept'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Kept");
        let resources: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='simulated_policy_resources'", [], |r| r.get(0)).unwrap();
        assert_eq!(resources, 0);
    }

    #[test]
    fn simulation_request_migration_failure_preserves_v4_data() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute_batch(include_str!("../migrations/0002_workspace.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        c.execute_batch(include_str!("../migrations/0003_sessions.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 3).unwrap();
        c.execute_batch(include_str!("../migrations/0004_simulated_policies.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", 4).unwrap();
        c.execute(
            "INSERT INTO profiles(id,name,description) VALUES('preserve','Preserve','existing')",
            [],
        )
        .unwrap();
        c.execute("CREATE TABLE simulated_policy_requests(sentinel TEXT)", [])
            .unwrap();
        c.execute("INSERT INTO simulated_policy_requests VALUES('keep')", [])
            .unwrap();
        assert!(storage::migrate(&mut c).is_err());
        let version: i64 = c
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let (name, sentinel): (String, String) = c.query_row(
            "SELECT p.name,r.sentinel FROM profiles p CROSS JOIN simulated_policy_requests r WHERE p.id='preserve'",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!((name.as_str(), sentinel.as_str()), ("Preserve", "keep"));
        let trigger_count: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'simulated_policy_%'", [], |row| row.get(0)).unwrap();
        assert_eq!(trigger_count, 0);
    }

    #[test]
    fn foreign_keys_are_enabled_on_each_configured_connection() {
        for _ in 0..2 {
            let c = Connection::open_in_memory().unwrap();
            storage::configure_connection(&c).unwrap();
            let enabled: i64 = c
                .pragma_query_value(None, "foreign_keys", |r| r.get(0))
                .unwrap();
            assert_eq!(enabled, 1);
            c.execute_batch("CREATE TABLE parent(id TEXT PRIMARY KEY); CREATE TABLE child(parent_id TEXT REFERENCES parent(id));").unwrap();
            assert!(
                c.execute("INSERT INTO child(parent_id) VALUES('missing')", [])
                    .is_err()
            );
        }
    }

    #[test]
    fn session_snapshot_is_immutable_and_sql_enforces_single_active_session() {
        let mut c = Connection::open_in_memory().unwrap();
        storage::migrate(&mut c).unwrap();
        let insert = "INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy) VALUES(?,'p',1,'{}',30,100,200,?,'timer_only')";
        c.execute(insert, rusqlite::params!["one", "active_work"])
            .unwrap();
        assert!(
            c.execute(insert, rusqlite::params!["two", "active_break"])
                .is_err()
        );
        assert!(
            c.execute("UPDATE sessions SET snapshot_json='{}' WHERE id='one'", [])
                .is_err()
        );
        c.execute("UPDATE sessions SET state='completed' WHERE id='one'", [])
            .unwrap();
        c.execute(insert, rusqlite::params!["two", "active_work"])
            .unwrap();
    }

    #[test]
    fn concurrent_sqlite_connections_cannot_create_two_active_sessions() {
        use std::{
            sync::{Arc, Barrier},
            thread,
            time::{SystemTime, UNIX_EPOCH},
        };
        let path = std::env::temp_dir().join(format!(
            "focusmimi-concurrency-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut first = Connection::open(&path).unwrap();
        storage::migrate(&mut first).unwrap();
        let second = Connection::open(&path).unwrap();
        storage::configure_connection(&second).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let run = |mut connection: Connection, id: &'static str, barrier: Arc<Barrier>| {
            thread::spawn(move || {
                barrier.wait();
                let tx = connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .unwrap();
                let active: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM sessions WHERE state IN ('active_work','active_break')",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                if active != 0 {
                    return false;
                }
                tx.execute("INSERT INTO sessions(id,profile_id,profile_version,snapshot_json,planned_duration_minutes,started_at_ms,deadline_at_ms,state,completion_policy) VALUES(?,'p',1,'{}',30,100,200,'active_work','timer_only')", [id]).unwrap();
                tx.commit().unwrap();
                true
            })
        };
        let a = run(first, "first", barrier.clone());
        let b = run(second, "second", barrier);
        let successes = a.join().unwrap() as u8 + b.join().unwrap() as u8;
        assert_eq!(successes, 1);
        let verify = Connection::open(&path).unwrap();
        let active: i64 = verify
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE state IN ('active_work','active_break')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 1);
        drop(verify);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rust_session_runtime_rechecks_approval_and_snapshots_profile_on_explicit_start() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let path = std::env::temp_dir().join(format!(
            "focusmimi-runtime-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = super::AppDatabase::open(&path).unwrap();
        let config = serde_json::json!({
            "name":"Deep work", "description":"Study", "activity":"Study", "intendedOutcome":"Complete notes",
            "durationMinutes":45, "taskId":null, "projectId":null,
            "applicationPolicy":{"mode":"disabled","entries":[]},
            "websitePolicy":{"mode":"disabled","domains":[]},
            "networkPolicy":{"mode":"disabled","applications":[]},
            "completionPolicy":"task_confirmation", "plannedBreaks":[]
        });
        {
            let c = db.connection.lock().unwrap();
            c.execute("INSERT INTO profiles(id,name,description,visible,approved_version) VALUES('p','Deep work','Study',1,1)", []).unwrap();
            c.execute("INSERT INTO profile_versions(profile_id,version,status,config_json,approved_at) VALUES('p',1,'approved',?,'now')", [config.to_string()]).unwrap();
        }
        assert!(db.start("p".into(), 1, false).is_err());
        let started = db.start("p".into(), 1, true).unwrap();
        assert_eq!(serde_json::to_value(&started).unwrap()["snapshot"], config);
        assert_eq!(serde_json::to_value(&started).unwrap()["profileVersion"], 1);
        assert!(db.start("p".into(), 1, true).is_err());
        assert_eq!(
            serde_json::to_value(db.status().unwrap()).unwrap()["active"]["id"],
            serde_json::to_value(&started).unwrap()["id"]
        );
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn simulated_policy_journal_is_idempotent_exclusive_and_recovers_on_reopen() {
        use crate::policy_simulation::LifecycleState;
        use std::time::{SystemTime, UNIX_EPOCH};
        let path = std::env::temp_dir().join(format!(
            "focusmimi-sim-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = super::AppDatabase::open(&path).unwrap();
        let config = serde_json::json!({
            "name":"Deep work", "description":"Study", "activity":"Study", "intendedOutcome":"Complete notes",
            "durationMinutes":45, "taskId":null, "projectId":null,
            "applicationPolicy":{"mode":"whitelist","entries":["editor.exe"]},
            "websitePolicy":{"mode":"disabled","domains":[]},
            "networkPolicy":{"mode":"disabled","applications":[]},
            "completionPolicy":"task_confirmation", "plannedBreaks":[]
        });
        {
            let c = db.connection.lock().unwrap();
            c.execute("INSERT INTO profiles(id,name,description,visible,approved_version) VALUES('p','Deep work','Study',1,1)", []).unwrap();
            c.execute("INSERT INTO profile_versions(profile_id,version,status,config_json,approved_at) VALUES('p',1,'approved',?,'now')", [config.to_string()]).unwrap();
        }
        let session = db.start("p".into(), 1, true).unwrap();
        let session_value = serde_json::to_value(&session).unwrap();
        let session_id = session_value["id"].as_str().unwrap().to_owned();
        let session_revision = session_value["revision"].as_u64().unwrap();
        let snapshot: String = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT snapshot_json FROM sessions WHERE id=?",
                [&session_id],
                |r| r.get(0),
            )
            .unwrap();
        let prepare_request: super::PolicySimulationRequest = serde_json::from_value(serde_json::json!({
            "action":"prepare", "sessionId":session_id, "expectedProfileVersion":1,
            "expectedRevision":session_revision, "requestId":"test-request-1"
        })).unwrap();
        let first = super::simulate_session_policy_inner(prepare_request, &db).unwrap();
        assert_eq!(first.plan.snapshot_json, snapshot);
        assert_eq!(first.plan.rules.len(), 1);
        let duplicate = db
            .create_policy_simulation(&session_id, 1, session_revision, "test-request-1")
            .unwrap();
        assert_eq!(duplicate, first);
        assert!(
            db.apply_simulated_policy(
                &first.operation_id,
                "test-request-1",
                first.revision,
                session_revision
            )
            .is_err()
        );
        assert!(
            db.create_policy_simulation(&session_id, 1, session_revision, "test-request-2")
                .is_err()
        );
        let apply_request: super::PolicySimulationRequest = serde_json::from_value(serde_json::json!({
            "action":"apply", "operationId":first.operation_id, "requestId":"test-apply-1",
            "expectedOperationRevision":first.revision, "expectedSessionRevision":session_revision
        })).unwrap();
        let applied = super::simulate_session_policy_inner(apply_request, &db).unwrap();
        assert_eq!(applied.state, LifecycleState::Applied);
        assert_eq!(applied.revision, 3); // planned -> applying -> applied
        assert_eq!(
            db.apply_simulated_policy(
                &first.operation_id,
                "test-apply-1",
                first.revision,
                session_revision
            )
            .unwrap(),
            applied
        );
        assert!(
            db.apply_simulated_policy(
                &first.operation_id,
                "test-apply-stale",
                first.revision,
                session_revision
            )
            .is_err()
        );
        let verify_request: super::PolicySimulationRequest = serde_json::from_value(serde_json::json!({
            "action":"verify", "operationId":first.operation_id, "requestId":"test-verify-1",
            "expectedOperationRevision":applied.revision, "expectedSessionRevision":session_revision
        })).unwrap();
        let verified = super::simulate_session_policy_inner(verify_request, &db).unwrap();
        assert_eq!(verified.state, LifecycleState::Verified);
        assert_eq!(
            db.verify_simulated_policy(
                &first.operation_id,
                "test-verify-1",
                applied.revision,
                session_revision
            )
            .unwrap(),
            verified
        );
        let rollback_request: super::PolicySimulationRequest = serde_json::from_value(serde_json::json!({
            "action":"rollback", "operationId":first.operation_id, "requestId":"test-rollback-1",
            "expectedOperationRevision":verified.revision
        })).unwrap();
        let rolled = super::simulate_session_policy_inner(rollback_request, &db).unwrap();
        assert_eq!(rolled.state, LifecycleState::RolledBack);
        assert_eq!(
            db.rollback_simulated_policy(&first.operation_id, "test-rollback-1", verified.revision)
                .unwrap(),
            rolled
        );
        let second = db
            .create_policy_simulation(&session_id, 1, session_revision, "test-request-3")
            .unwrap();
        {
            let mut connection = db.connection.lock().unwrap();
            crate::policy_simulation_store::begin_interrupted_apply(
                &mut connection,
                &second.operation_id,
                "test-interrupted-apply",
                second.revision,
                session_revision,
                50,
            )
            .unwrap();
        }
        assert_eq!(
            db.get_policy_simulation(&second.operation_id)
                .unwrap()
                .state,
            LifecycleState::Applying
        );
        drop(db);

        let reopened = super::AppDatabase::open(&path).unwrap();
        let recovered = reopened
            .get_policy_simulation(&second.operation_id)
            .unwrap();
        assert_eq!(recovered.state, LifecycleState::RolledBack);
        assert!(recovered.simulated_resources.is_empty());
        let event_count: i64 = reopened
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM simulated_policy_events WHERE operation_id=?",
                [&second.operation_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(event_count, recovered.revision as i64);
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }
}
