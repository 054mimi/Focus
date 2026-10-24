# FOCUS by MIMI — Desktop development

This repository contains the non-privileged native application foundation,
Phase 2 dashboard/profile management, and the Phase 3 logical session timer.
Phase 3 does not implement policy enforcement, create a Windows service, or
change firewall rules, privileges, or Windows security settings.

## Verified local prerequisites

Verified on 2026-10-23:

- Windows 11, Node.js 24.21.0, npm 11.19.0, Git.
- Rust stable `x86_64-pc-windows-msvc`, rustc 1.99.1, Cargo 1.99.1.
- Microsoft Visual Studio Build Tools 18 with the MSVC x64 C++ component and
  Windows SDK 10.0.26100.0.
- Microsoft Edge WebView2 Runtime 155.0.4283.62.

The only local environment adjustment still needed for ordinary terminals is
to add `%USERPROFILE%\.cargo\bin` to PATH (or use a Rust-enabled terminal).
The MSVC tools require a Visual Studio Developer Command Prompt/PowerShell so
`cl.exe` and the linker are on PATH. The Windows SDK and WebView2 runtime are
already present on this machine; verify them on other systems. Creating an
NSIS installer additionally requires NSIS (`makensis.exe`); Phase 1 development
and the executable build do not require packaging an installer.

## Phase 2 capabilities

- Monochrome, theme-adaptive `FOCUS — MIMI` SVG wordmark and adaptive light,
  dark, and system themes.
- Dashboard navigation for overview, profiles, tasks, and projects.
- SQLite-backed profile proposals and approved versions. Existing versions are
  retained, edits remain proposals until the user approves them, profiles can
  be duplicated, renamed through proposals, deleted, shown or hidden on the
  dashboard, and reordered. Proposed fields are compared with the approved
  version before explicit approval.
- Profile data includes activity, intended outcome, duration, application,
  website, per-application network, and completion policy fields. These are
  configuration data only; the app does not enforce them.
- SQLite tables for projects and tasks.

Rename and delete actions are handled through a reviewed profile proposal or
explicit deletion. Tasks and projects currently support creation and listing;
full editing and completion workflows, profile import/export,
history/statistics, and the separate web dashboard remain future work. Never
interpret stored policy values as currently applied controls.

## Phase 3 session timer

- Rust owns session transitions. SQLite stores UTC start/deadline timestamps,
  the exact approved profile snapshot, revisions, and transition events. The
  UI polls Rust and is not timer authority.
- A pre-session review shows the approved settings and requires explicit
  confirmation. Rust rechecks the exact approved visible profile version.
- Only one `active_work`/`active_break` session may exist. Rust uses an
  immediate transaction and the database has a partial unique index.
- Breaks are manually started because profile breaks have no scheduled
  offsets. A break never pauses or extends the session deadline.
- `timer_only` completes at expiry; `task_confirmation` waits for a direct
  outcome. No confirmation is inferred. Neither state activates restrictions.
- See `docs/session-engine.md` for clock change, sleep, restart, and recovery
  limits. After app restart or reboot, UTC wall time is the only persisted
  clock source; a clock change while closed can alter apparent remaining time.
- Tauri app commands are enumerated in `src-tauri/build.rs`; `tauri-build`
  generates per-command `allow-*` permissions, and the main-window capability
  lists those exact permissions. Tauri's default `permissions: []` alone would
  not block custom registered commands.

Session schema migration `0003` is additive and transactional. The app enables
and verifies SQLite foreign keys on every opened connection. Migration failure
leaves version 2 and existing data intact. The timer still needs a native
runtime invocation check; compilation and ACL generation do not prove IPC
works in a launched window.

## Phase 4A/4B simulated policy lifecycle

The Rust simulator validates policy descriptors from the immutable active
session snapshot and stores lifecycle events and fake resources in SQLite
schema version 5. Migration `0005` adds persistent request IDs, saved response
payloads, and database transition/revision guards. IDs are globally unique in
the local database; a duplicate with the same action/payload returns its
saved result, while conflicting reuse or stale revisions are rejected. The
unprivileged `simulate_session_policy` command supports prepare, apply,
verify, rollback, and read-only status; it does not call Windows APIs or
activate policy. Apply/resource/event/response persistence is transactional.
Timeouts and partial/verify/rollback failures are deterministic test inputs,
not wall-clock waits. Concurrency, migration rollback, persistence failure,
idempotency, stale session checks, and restart recovery are covered by Rust
tests. The command is included in the generated app-command ACL and the
default main-window capability. This ACL grant is WebView-to-app authorization
only; it does not authenticate a future service. The UI explicitly says that
all restrictions remain inactive. Generated ACL verification succeeded; a
launched native WebView IPC call remains unverified. See
`docs/phase-4a-simulation-design.md` for the lifecycle and future IPC boundary.

## Install and run

From the repository root:

```powershell
npm ci
npm run tauri:dev
```

`npm ci` uses `package-lock.json`. Rust crates are pinned/resolved through
`src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`. Tauri CLI is project-local;
no global CLI install is required.

## Checks

```powershell
npm run typecheck
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
```

For Rust checks, run from a Visual Studio Developer shell with the Rust
toolchain in PATH. The SQLite schema migration opens a local application data
database on first launch; the test suite uses in-memory SQLite. No privileged
component, service, firewall rule, or security setting is created or changed
by this phase.

## Dependency policy

`package.json` uses exact versions and `package-lock.json` records the npm
dependency graph. Rust direct dependencies use exact versions; `Cargo.lock`
pins their transitive graph. Update dependencies deliberately, review security
and compatibility notes, then rerun all checks. SQLite is built using the
`rusqlite` bundled feature so developers do not need a separately installed
SQLite library.
