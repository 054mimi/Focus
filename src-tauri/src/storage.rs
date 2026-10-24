use rusqlite::{Connection, TransactionBehavior};

const INITIAL_SCHEMA: &str = include_str!("../migrations/0001_initial.sql");
const WORKSPACE_SCHEMA: &str = include_str!("../migrations/0002_workspace.sql");
const SESSION_SCHEMA: &str = include_str!("../migrations/0003_sessions.sql");
const SIMULATED_POLICY_SCHEMA: &str = include_str!("../migrations/0004_simulated_policies.sql");
const SIMULATED_POLICY_REQUEST_SCHEMA: &str =
    include_str!("../migrations/0005_simulation_request_journal.sql");

pub fn configure_connection(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let enabled: i64 = connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if enabled != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

pub fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    configure_connection(connection)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 5 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(INITIAL_SCHEMA)?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(WORKSPACE_SCHEMA)?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 2 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SESSION_SCHEMA)?;
        transaction.pragma_update(None, "user_version", 3)?;
        transaction.commit()?;
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 3 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SIMULATED_POLICY_SCHEMA)?;
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 4 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SIMULATED_POLICY_REQUEST_SCHEMA)?;
        transaction.pragma_update(None, "user_version", 5)?;
        transaction.commit()?;
    }
    Ok(())
}
