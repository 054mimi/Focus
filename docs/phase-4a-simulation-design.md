# Phase 4A: simulated policy lifecycle design

This is a local, non-enforcing simulation. Phase 4A adds no Windows policy
calls, elevated process, service, broker, installer, privileged endpoint, or
claim that a session is protected. The UI's `NOT ENFORCING` status remains
truthful. The simulator is a fake adapter backed by Phase 4A-owned SQLite
tables; its records and fake resources are separate from real OS policy state.

## Boundaries and authority

The React UI remains untrusted. The existing unprivileged Rust/Tauri process
is authoritative for session state, deadlines, approved profile snapshot,
policy plan validation, simulation requests, and simulation journal. SQLite
holds the session snapshot and simulated records. The simulator reads the
immutable snapshot attached to the selected active session; it never reads
live profile edits to alter a running session. One narrowly scoped,
unprivileged `simulate_session_policy` Tauri command exposes prepare, apply,
verify, rollback, and read-only status. Rust loads the snapshot from SQLite
and validates session/version/revisions; the WebView cannot submit policy
rules or fault injection. The command only mutates or reads simulation data
and performs no OS calls.

The future enforcement-component boundary remains design-only. Its eventual
placement, identity, privilege, transport, lifecycle, policy ownership, and
session-authority reconciliation are not approved here. There is no privileged
process or endpoint to authenticate in Phase 4A.

## Future IPC contract (design only)

If separately approved, define a versioned, length-bounded message envelope
with protocol version, request ID, operation, session ID, exact profile
version, and expected session revision. Authenticate the OS caller identity
on each connection using the selected local transport's OS security context;
do not trust a client-supplied SID, shared secret in the UI, localhost, or
Tauri capability as service authentication. Authorize only the minimum fixed
operations. Reject unknown versions/fields, malformed or oversized payloads,
unapproved or mismatched snapshots, stale revisions, duplicate IDs with
different payloads, and unauthorized callers. Repeating the same request ID
and payload must return its original result without replaying side effects.
Never expose shell, executable, arbitrary path, or raw firewall commands.

This is a set of protocol requirements, not a selected or implemented
transport. A Tauri app ACL controls WebView access to app commands; it does
not authenticate a future service client.

## Policy lifecycle and journal

The plan is derived from application, website, and per-application network
policy entries in the exact active session snapshot. It contains only bounded,
validated descriptors and is sorted deterministically. Duplicate resource
keys, unsupported modes, empty enabled policies, malformed entries, inactive
sessions, and stale session revisions are rejected. SQLite assigns each
operation a random 128-bit ID; a caller request key is unique and idempotent.
One nonterminal simulated operation per session is allowed. Operation IDs are
independent random 128-bit SQLite values prefixed with `sim-`. Each mutating
command has its own 128-bit Web Crypto request ID (`ui-` plus 32 hex digits),
stored globally unique in `simulated_policy_requests`, independent of session
or operation revision. The request table persists action, canonical payload,
and serialized result. Request IDs are idempotency keys, not authentication
credentials.

Allowed transitions:

```text
planned -> applying -> applied -> verified
                    \-> rollback_pending -> rolled_back
applied ----------------> rollback_pending -> rolled_back
verified ---------------> rollback_pending -> rolled_back
applying ----------------> failed
rollback_pending --------> failed
planned -----------------> rollback_pending -> rolled_back
```

`failed` is terminal and never means applied or verified. In Phase 4B each
apply, verify, and rollback request, its state/events, fake resource changes,
and serialized response are committed in one `BEGIN IMMEDIATE` transaction.
The journal records the internal `applying` transition before `applied` in
revision order within that transaction. A process crash before commit leaves
the previous committed state and fake resources intact. Persistence failure
rolls back the whole action and cannot return success. Verification compares
the simulated resources with the plan. A verification failure moves to
`rollback_pending`; a rollback failure or timeout records `failed` and leaves
only the fake state visibly uncertain. Events are append-only, contiguous,
and match the operation revision. SQL triggers and Rust checks enforce legal
transitions and one-step revision increments.

Reusing a request ID with the same action and exact payload returns its saved
serialized result without another event or resource write. Reusing it with a
different action or payload is a conflict. A new request ID with a stale
operation revision is rejected. Prepare request IDs are unique across the
database, not merely within a session. Migration `0005` adds the request
journal and transition guards; Phase 4A operations are backfilled with their
original planned response without changing their operation/event records.

Prepare validates an active session and exact approved profile version,
snapshot, and expected session revision. Apply and verify repeat these checks
inside the write transaction. Rollback requires the current operation
revision but is permitted after the session ends because it only restores
fake local resources. A planned operation may be rolled back to release its
slot when a later action becomes stale.

On app database initialization, legacy/interrupted `applying` and
`rollback_pending` operations are recovered by restoring their saved prior
**simulated** resource map in a transaction and recording `rolled_back`.
Malformed recovery data becomes `failed` without claiming cleanup. A database
error rolls back that recovery transaction so the next initialization can
retry. `applied` remains unverified; recovery never promotes it to `verified`.
Phase 4B's simulated apply itself is atomic, so a crash during its transaction
leaves the prior committed state rather than a partially written fake map.

The fake adapter only mutates the simulation tables. The IPC lifecycle uses
the success path; pre-apply failure, timeout before/after partial apply,
verification failure/timeout, rollback failure/timeout, and restart recovery
are deterministic Rust test inputs with no elapsed-time wait. No production
fault input exists. No OS API, firewall state, Windows service, or system
policy is involved.

## Failure and concurrency behavior

All journal transitions use `BEGIN IMMEDIATE`, unique request IDs,
operation-revision compare-and-set, legal-state validation, and
one-active-operation-per-session constraints. SQLite serializes competing
connections. Conflicting operations fail without touching fake resources.
Exact retries for prepare/apply/verify/rollback return their first saved
result; a new request ID can repeat an idempotent action only against the
current operation revision. Out-of-order transitions fail. Apply/verify
failure is never reported as success. Partial apply is explicit and requires
rollback; startup recovery attempts it automatically. If SQLite fails, the
operation remains in its last committed state and reports the failure.

## Limits and future approval gate

The dashboard lets the user run the success-path prepare/apply/verify flow and
roll back the fake operation. It prevents concurrent duplicate clicks, shows
backend rejection/errors, restores the last operation ID on reload, and
always displays that restrictions remain inactive. It does not claim to test
actual policy effects.

Simulation tests lifecycle logic and journal recovery only. It does not prove
IPC authentication, OS policy effectiveness, firewall preservation, service
security, tamper resistance, or behavior after real Windows crashes. It
cannot resist an administrator who edits the SQLite database or application.
Generated Tauri ACL inspection verifies command registration and the
main-window permission mapping, but actual WebView-to-Rust IPC has not been
verified in a launched native window.
Before any real enforcement work, separately approve the component type and
authority model, transport and caller-identity design, privilege and lifecycle
requirements, recovery semantics, and a disposable Windows 11 VM test plan.
Real OS policy writes require a separate, explicit scope and isolated tests.
