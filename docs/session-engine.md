# Phase 3 session engine decisions

This document records the Phase 3 design before the session implementation.
The engine is a local, logical timer owned by the unprivileged Rust/Tauri
process. It is **not** enforcement and cannot keep restrictions active after
the process exits.

## State machine

Pre-session review exists only in the UI and creates no session row. The user
must explicitly confirm the reviewed approved version. Rust rechecks that the
profile exists, is visible, remains approved at the exact version, and its
snapshot is valid inside the start transaction.

```text
review (ephemeral) --confirm--> active_work
active_work --start next configured break--> active_break
active_break --break deadline / skip--> active_work
active_work or active_break --session deadline, timer_only--> completed
active_work or active_break --session deadline, task_confirmation--> awaiting_completion
awaiting_completion --explicit completed/partial/not_completed--> completed
```

There is no pre-deadline completion or ordinary cancellation transition. The
break list has durations but no scheduled offsets. Therefore a user may start
the next configured break explicitly while working. Starting a break consumes
that entry; skipping is permitted only by ending an active break early. The
session deadline continues through breaks, and a break deadline is clamped to
the session deadline. Expiry takes precedence over break completion. Break
application/site/network settings are retained in the snapshot, but not
applied. This phase does not claim any restriction is enforced.

`active_work` and `active_break` are the only active states. A partial unique
SQLite index over those states enforces one active session; Rust rejects a
second start before insert as well. `awaiting_completion` is a non-active,
timer-expired record and does not block a new session. Missing confirmation
never starts another session and cannot keep the computer restricted because
there is no enforcement in Phase 3.

## Deadlines, clock and recovery

Persist UTC epoch milliseconds for `started_at`, `deadline_at`, and transition
deadlines. During a process lifetime, compare wall elapsed time with monotonic
elapsed time and use the greater elapsed duration. This prevents a backward
wall-clock adjustment from extending a running session and counts sleep when
either clock advances. A discrepancy is recorded as a clock anomaly. A forward
wall-clock jump can therefore expire a session early.

Monotonic anchors cannot survive process termination or reboot. On process
startup/recovery, persisted UTC deadline and the current wall clock are the
only available comparison. A backward clock change while the app is closed can
extend the apparent remaining time; a forward change can expire it early. The
engine reconciles on app startup and before session reads/transitions, records
expiry once, and never restarts the original duration. Sleep/wake while the
process remains alive is reconciled at the next command. Shutdown, a hung
process, battery loss, OS clock manipulation, and administrator changes cannot
be guaranteed against without an external trusted authority. There is no
background service in this phase.

Time is stored and compared in integer milliseconds. Adjustment requests use
whole remaining minutes; conversion is checked for overflow. Eligibility uses
the exact current milliseconds; the greater-than-60-minute half rule rounds
its minimum upward to the next whole minute. Requests must shorten to a
positive value. A request equal to the exact current remaining milliseconds
is an idempotent no-op with no revision/event (including 15→15 and 10→10);
other equal whole-minute requests that would extend by rounding are rejected.
Extensions are not implemented. Every accepted adjustment is an event and updates the deadline
and revision atomically. A retry using an old revision is stale. Separate
sequential requests are evaluated against the then-current remaining time; no
request can reuse the original duration.

## Completion and breaks

At deadline, `timer_only` records `completed` with a timer-expiry event.
`task_confirmation` records `awaiting_completion`; only an explicit user
response of `completed`, `partial`, or `not_completed` transitions it to
`completed`. Timer expiry and task outcome remain distinct fields/events.
There is no automatic positive completion response.

Breaks are bounded by each approved snapshot's preconfigured list. Because
the current profile schema does not specify break start offsets, entry is
explicit rather than automatic. A break countdown ends at the earlier of its
own deadline and the session deadline. On break deadline, return to work if
the session remains unexpired; otherwise apply session completion. Skipping
ends only the current break and does not change the session deadline or
remaining duration. A break cannot create another session or extend time.

## Immutable snapshot and persistence

At start, copy the complete JSON configuration from the exact approved
`profile_versions` row into `sessions.snapshot_json`, with `profile_id`,
`profile_version`, and `planned_duration_minutes`. That snapshot is the source
for review/history and is protected against SQL updates by a trigger. Profile
edits/deletion after start cannot change it. Session events record start,
shortening, break start/end/skip, expiry, and task outcome with UTC timestamps,
revision, and compact event data.

All state changes use `BEGIN IMMEDIATE` transactions. The single process
serializes operations through its SQLite mutex; the unique partial index and
revision check protect against a second connection/process and stale retries.
Foreign keys are enabled immediately on every runtime connection. Migration
`0003` is additive and transactional; failure leaves schema version 2 and its
user data intact.

Generated local identifiers use SQLite's 128-bit `randomblob(16)` output in
place of the former timestamp-only fallback. Generated-ID inserts use plain
`INSERT` so an extraordinarily unlikely primary-key collision returns an
error instead of updating an unrelated existing record.

## Tauri command access

Tauri v2 permits registered app commands by default when no app ACL manifest
is generated; an empty capability permission list alone does not disable
those custom commands. Phase 3 declares the exact command set in `build.rs`
with `AppManifest::commands` and enumerates its generated `allow-*`
permissions in the main-window capability. No plugin or privileged commands
are added. Successful compilation validates ACL generation/configuration, but
a native launch and invocation still need a runtime check.
