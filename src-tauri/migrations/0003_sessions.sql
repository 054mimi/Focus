CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL,
    profile_id TEXT NOT NULL,
    profile_version INTEGER NOT NULL CHECK (profile_version > 0),
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    planned_duration_minutes INTEGER NOT NULL CHECK (planned_duration_minutes BETWEEN 5 AND 1440),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    deadline_at_ms INTEGER NOT NULL CHECK (deadline_at_ms > started_at_ms),
    state TEXT NOT NULL CHECK (state IN ('active_work', 'active_break', 'awaiting_completion', 'completed')),
    completion_policy TEXT NOT NULL CHECK (completion_policy IN ('timer_only', 'task_confirmation')),
    completion_outcome TEXT CHECK (completion_outcome IN ('completed', 'partial', 'not_completed')),
    break_index INTEGER NOT NULL DEFAULT 0 CHECK (break_index >= 0),
    break_deadline_at_ms INTEGER,
    expired_at_ms INTEGER,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    clock_anomaly_delta_ms INTEGER,
    CHECK ((state = 'active_break') = (break_deadline_at_ms IS NOT NULL)),
    CHECK (expired_at_ms IS NULL OR expired_at_ms >= started_at_ms),
    CHECK (completion_outcome IS NULL OR completion_policy = 'task_confirmation')
);

CREATE UNIQUE INDEX one_active_session
    ON sessions ((1))
    WHERE state IN ('active_work', 'active_break');

CREATE TABLE session_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'started', 'time_shortened', 'break_started', 'break_ended',
        'break_skipped', 'deadline_expired', 'task_outcome_recorded',
        'clock_discrepancy'
    )),
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    detail_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(detail_json))
);
CREATE INDEX session_events_session_id ON session_events(session_id, id);

CREATE TRIGGER session_snapshot_immutable
BEFORE UPDATE OF profile_id, profile_version, snapshot_json, planned_duration_minutes
ON sessions
BEGIN
    SELECT RAISE(ABORT, 'session profile snapshot is immutable');
END;
