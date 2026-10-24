CREATE TABLE simulated_policy_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    request_key TEXT NOT NULL UNIQUE CHECK (length(request_key) BETWEEN 1 AND 128),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
    profile_version INTEGER NOT NULL CHECK (profile_version > 0),
    session_revision INTEGER NOT NULL CHECK (session_revision >= 0),
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    plan_json TEXT NOT NULL CHECK (json_valid(plan_json)),
    prior_resources_json TEXT NOT NULL CHECK (json_valid(prior_resources_json)),
    state TEXT NOT NULL CHECK (state IN ('planned','applying','applied','verified','rollback_pending','rolled_back','failed')),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    error TEXT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
);

CREATE UNIQUE INDEX one_nonterminal_simulation_per_session
    ON simulated_policy_operations(session_id)
    WHERE state IN ('planned','applying','applied','verified','rollback_pending');

CREATE TABLE simulated_policy_resources (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
    resource_key TEXT NOT NULL,
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    PRIMARY KEY (session_id, resource_key)
);

CREATE TABLE simulated_policy_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL REFERENCES simulated_policy_operations(operation_id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL,
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    detail TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX simulated_policy_event_revision
    ON simulated_policy_events(operation_id, revision);
