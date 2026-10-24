CREATE TABLE simulated_policy_requests (
    request_id TEXT PRIMARY KEY NOT NULL CHECK (length(request_id) BETWEEN 1 AND 128),
    operation_id TEXT REFERENCES simulated_policy_operations(operation_id) ON DELETE RESTRICT,
    action TEXT NOT NULL CHECK (action IN ('prepare','apply','verify','rollback','status')),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    error_text TEXT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CHECK ((result_json IS NOT NULL AND error_text IS NULL) OR (result_json IS NULL AND error_text IS NOT NULL))
);

CREATE INDEX simulated_policy_requests_by_operation
    ON simulated_policy_requests(operation_id, created_at_ms);

CREATE TRIGGER simulated_policy_state_transition_guard
BEFORE UPDATE OF state ON simulated_policy_operations
WHEN NEW.state <> OLD.state AND NOT (
    (OLD.state = 'planned' AND NEW.state IN ('applying','rollback_pending')) OR
    (OLD.state = 'applying' AND NEW.state IN ('applied','rollback_pending','rolled_back','failed')) OR
    (OLD.state = 'applied' AND NEW.state IN ('verified','rollback_pending')) OR
    (OLD.state = 'verified' AND NEW.state = 'rollback_pending') OR
    (OLD.state = 'rollback_pending' AND NEW.state IN ('rolled_back','failed'))
)
BEGIN
    SELECT RAISE(ABORT, 'invalid simulated policy state transition');
END;

CREATE TRIGGER simulated_policy_revision_guard
BEFORE UPDATE ON simulated_policy_operations
WHEN (NEW.state <> OLD.state AND NEW.revision <> OLD.revision + 1)
  OR (NEW.state = OLD.state AND NEW.revision <> OLD.revision)
BEGIN
    SELECT RAISE(ABORT, 'invalid simulated policy revision');
END;

CREATE TRIGGER simulated_policy_event_revision_guard
BEFORE INSERT ON simulated_policy_events
WHEN NEW.revision <> (SELECT revision FROM simulated_policy_operations WHERE operation_id = NEW.operation_id)
BEGIN
    SELECT RAISE(ABORT, 'simulated policy event revision mismatch');
END;
