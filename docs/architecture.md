# FOCUS by MIMI — architecture and implementation plan

**Tagline:** Commit. Focus. Complete.

Use **FOCUS by MIMI** as the user-facing name in the application, window,
widget, notifications, installer, and user-facing documentation. Use
`focusmimi` for the repository, technical package identifiers, and executable
name (`focusmimi.exe`).

This document establishes the Phase 0 architecture for the Windows-first,
local-first FOCUS by MIMI application. It is a plan, not a claim that any
enforcement or desktop functionality has already been implemented.

## Repository inspection (2026-10-23)

- The repository has no commits and the `main` branch has no HEAD.
- The checkout contains three staged documentation files: `README.md`,
  `README-plain.txt`, and `README-text-version.md`. They are preserved as-is.
- No application source, package manifest, Rust workspace, migrations, or
  tests are present.
- Node.js 24.21.0 and Git are available. Rust/Cargo, Tauri CLI, MSVC C++ build
  tools (`cl.exe`), and CMake were not found on PATH. The npm version and
  installed Windows SDK/Tauri prerequisites remain unverified.
- No installed dependency versions can be inferred because there is no
  package manifest or lockfile.

The Phase 1 Windows build prerequisite set must be verified before selecting
framework versions: Node.js/npm, stable Rust with the MSVC Windows target,
Microsoft C++ Build Tools with **Desktop development with C++**, and a Windows
SDK. Verify the Edge WebView2 runtime on the target machine (it is normally
present on Windows 11). CMake was not found during inspection, but it is not
identified as a standard Tauri prerequisite; require it only if a chosen
dependency or build step demonstrates a need. The Tauri CLI should be pinned
in the project manifest/lockfile rather than assumed globally installed.

The absence of a Rust toolchain blocks a runnable Tauri milestone today. The
safe first deliverable is this architecture and acceptance plan. Do not install
services, elevate, or change firewall/system policy as part of setup.

## Proposed repository layout

```text
src/                         React + TypeScript UI
src-tauri/                   Tauri shell and unprivileged Rust application layer
crates/domain/               Platform-independent domain types and rules
crates/storage/              SQLite schema, migrations, repositories, backups
crates/enforcement-protocol/  Narrow versioned IPC contracts
windows/enforcer/             Separately installed privileged component (later)
docs/                         Architecture, threat model, testing, recovery, decisions
```

The privileged component is deliberately a separate boundary. Whether it is a
Windows service, a narrowly privileged broker, or a combination is an open
design decision to resolve with a working threat model and a controlled
prototype. The UI must never receive arbitrary administrator shell access.

## Component responsibilities and trust boundaries

### Complete product surfaces

The first product surface is the native Windows desktop application: a
dashboard, reusable and versioned profiles, explicit pre-session review,
active-session and completion views, a secondary resizable widget, themes,
tasks/projects, history/statistics, local data and recovery, and verified
Windows enforcement. Profiles expose whitelist and blacklist application
policies, website/domain filtering, and per-application network allow/block
policies to the extent verified on the target Windows configuration. A test
mode must report capability and limitations without claiming a strict lock.

The browser dashboard is a separate, later product surface. It may support
approved non-privileged profile, task/project, history, statistics, and
preference operations; it is not a web replacement for native enforcement and
is not part of initial-release implementation.

1. **React, TypeScript, Tailwind, Tauri UI:** edits proposals and preferences,
   presents the pre-session review, requests explicit confirmation, and
   displays service-reported state. The UI is untrusted and is never the timer,
   authorization, or enforcement authority.
2. **Unprivileged Rust app layer:** validates domain requests, owns application
   state transitions, accesses SQLite, and communicates using a narrow,
   versioned IPC contract. It cannot submit arbitrary commands or treat UI
   flags/database values as authorization.
3. **Privileged Windows enforcement component:** accepts only authenticated,
   validated, narrowly scoped policy operations; owns authoritative deadlines
   and active policy state; applies, verifies, audits, and reverses only
   FOCUS-owned resources. It reports applied, verified, partial, failed, and
   recovered states distinctly.
4. **SQLite local store:** stores profile versions and approvals, tasks,
   projects, settings, sessions, events, and recovery metadata. Use migrations,
  transactions, constraints, and backups. Define one authoritative session
  state owner and a transactional, reconciled persistence protocol between it
  and SQLite; do not let UI and service independently claim authority. The
  service's authoritative session record must survive UI restarts; do not make
  a user-editable field proof of authorization. Test corrupt/unreadable data,
  interrupted migrations/upgrades, validated restore, and rollback/recovery.
5. **Future web dashboard:** a separate presentation surface, considered only
   after native stabilization and approval. It uses explicitly designed,
   authenticated interfaces and never writes the database directly or invokes
   privileged commands. Localhost alone is not an authorization boundary.

### Initial data concepts

`Profile`, immutable `ProfileVersion`, `ProfileApproval`, `Task`, `Project`,
`Session`, `SessionEvent`, `TimerAdjustment`, `BreakSegment`, `EnforcementIncident`,
`Setting`, and `RecoveryRecord`. Session completion, task completion, and policy
cleanup are separate facts. A session snapshots its approved profile version;
editing an inactive profile cannot mutate that snapshot.

Profiles also carry dashboard/widget visibility and ordering. Profile
import/export must validate schema and platform-specific paths, exclude
unnecessary personal data, and never apply privileged policy or start a
session on import. History records planned/actual duration, profile version,
completion response, timer adjustments, breaks, enforcement incidents, and
final status. Statistics distinguish elapsed time, task outcome, and verified
enforcement.

## Session and policy decisions

### Phase 3 implementation status (2026-10-23)

Phase 3 implements a local logical session timer in the unprivileged Rust/Tauri
process. Rust owns session transitions; SQLite stores UTC deadlines, immutable
approved-profile snapshots, and events. A partial unique index and Rust
transaction checks enforce one active session. This phase adds no service,
broker, privilege change, firewall operation, or enforcement claim. Because a
desktop process and Windows wall clock cannot guarantee continuity through
shutdown, termination, or administrator clock changes, Phase 3's deadline
recovery is explicitly best-effort after restart; the Phase 0/4 enforcement
component remains a future design and the source of strict enforcement
authority if that feature is approved and implemented.

The concrete rules and limitations are in `docs/session-engine.md`. At
deadline, timer-only sessions complete; task-confirmation sessions wait for an
explicit outcome. No outcome is inferred. The profile schema has no break
offset, so configured breaks begin only on user action and do not pause or
extend the session deadline. Timer shortening is bounded and revision-checked.
Active-session profile snapshots do not change when profile versions change.

### Active-session profile conflict (unresolved)

The specification asks for explicit approval of a profile change that can
apply to the current session, while also stating that remaining time is the
only ordinary setting changeable during a strict session. Those requirements
conflict if approval can change active application, website, or network rules.

**Recommended default for implementation:** active strict sessions use an
immutable approved policy snapshot. Profile edits create a proposed version
for a later session. During an active session, only service-validated time
adjustments are allowed. A separately named emergency recovery procedure may
release or repair enforcement, but is not an ordinary profile-edit path and
must be audited. Do not implement current-session policy mutation unless the
product owner explicitly approves a revised rule and its security consequences.

### Start, timer, completion, and breaks

Starting a strict session requires a valid approved profile, a free single
strict-session slot, a healthy enforcement component, safe recovery access,
policy preparation and verification, clear disclosure of unsupported controls,
and explicit user confirmation. If critical enforcement is unavailable, do
not label the session enforced; block strict start or offer a clearly weaker,
separately named mode only after product approval.

Deadlines are owned by the enforcement component, not JavaScript. Shortening
uses remaining time at request time: above 60 minutes, retain at least half;
from 30 through 60 minutes, retain at least 30; above 15 and below 30, retain
at least 15; at 15 or below, disallow shortening. Extensions require explicit
confirmation and an audit event. A timer-plus-task-confirmation session must
have a bounded release behavior if confirmation never arrives. Break policies
are fixed before commitment and remain disabled until their transitions can
be safely applied and verified.

Profiles choose either **timer-only** completion or **timer plus task
confirmation** (completed, partial, or not completed). Session end, task
outcome, and enforcement cleanup are separate records. Missing confirmation
must have a bounded, documented release policy and must never strand the
computer indefinitely. Use a persisted UTC deadline and monotonic elapsed-time
measurement while the OS is running; after sleep, reboot, or wall-clock change,
reconcile against the persisted deadline, record clock/recovery anomalies, and
never silently restart the original duration. Windows does not provide a
single clock source that is both monotonic across reboot and immune to
administrator clock changes, so this behavior and its limits require explicit
design and testing.

### Windows enforcement risks and limitations

- Windows Firewall executable-specific rules can support some per-program
  network controls, but rule precedence, services, child processes, path
  changes, IPv4/IPv6, and existing policy conflicts need validation. Explicit
  block rules can override allows, and GPO/MDM policy can disable local rule
  merging, so a successful API write does not prove an effective restriction.
  Preflight effective policy where supported and report conflicts or
  unverifiable state. Own and tag only FOCUS rules; persist their stable IDs
  and prior state where applicable; make apply/cleanup idempotent; remove only
  owned rules; verify cleanup; and never clear or rewrite unrelated firewall
  policy. If preservation or restoration cannot be verified, report it and
  enter the documented recovery path.
- DNS/domain filtering does not guarantee website blocking. Encrypted DNS,
  VPNs, proxies, caches, existing connections, shared hosting/CDNs, and IP
  changes affect coverage. Avoid broad IP blocks that break unrelated sites.
- Process detection/termination is not strong application control. Preventing
  relaunch, handling child processes/elevation, and protecting accessibility,
  security, and recovery tools require a separately reviewed approach.
- A standard service cannot defeat an unrestricted administrator. Document
  this limitation; administrators can change service configuration/permissions,
  stop the service, alter firewall policy, or replace application files. Service
  ACLs and authenticated IPC reduce accidental or ordinary-user abuse; they
  do not make administrator-controlled policy tamper-proof. Avoid invasive
  monitoring, keylogging, and credential collection.
- Application control must distinguish process detection, termination,
  ordinary relaunch prevention, and OS-managed application control. Windows
  does not guarantee that a user-mode service can stop every application launch
  or child process, and protections can vary by Windows edition/configuration.
  Whitelist mode must not be described as a hard OS-enforced allowlist without
  a separately selected and verified OS control.
- Service lifecycle requirements include explicit install/start/stop/update/
  uninstall permissions, startup/recovery configuration, versioned policy
  protocol, safe behavior during upgrade with an active session, health/status
  reporting, and a documented emergency recovery path. Tamper events are
  limited to relevant policy/service integrity signals; log minimally and
  locally, and make log failure visible.
- Partial apply, service restart, UI crash, reboot, sleep, database failure,
  upgrade, and rollback all need explicit recovery behavior and isolated
  integration tests. Do not test destructive system policies on an everyday
  machine.

## Phases and acceptance criteria

| Phase | Deliverable | Acceptance criteria |
|---|---|---|
| 0 | Repository inspection, architecture, threat/risk and decision records | Existing work preserved; architecture, boundaries, risks, conflicts, phases, and first milestone documented. |
| 1 | React/TypeScript/Tailwind + Tauri/Rust shell, themes, contracts, SQLite migration baseline | Reproducible Windows build; FOCUS by MIMI branding and `focusmimi` technical IDs; documented IPC/state boundaries; schema migration and automated baseline checks run. |
| 2 | Dashboard, profiles, proposed versions/approval, tasks/projects, local persistence | Profile edits produce reviewable versions; approved history persists; active session snapshots do not change. |
| 3 | Unprivileged Rust session engine, review, explicit commitment, timer, completion, break model | Rust/SQLite-authoritative local session state and deadline; exact timer boundaries and best-effort recovery limits covered by tests; no frontend-only timer authority. |
| 4 | Separately designed enforcement component lifecycle, authenticated IPC, policy apply/verify/rollback, logging | Enforcement authority moves only after a separately reviewed component is implemented; narrow test policy works in an isolated environment; permissions/lifecycle and recovery are documented; state is accurately reported; restart and partial-failure recovery demonstrated. |
| 5 | Application whitelist and blacklist policies | Controlled tests distinguish detection, termination, relaunch control, and OS-managed controls; limitations and essential/recovery access are verified. |
| 6 | Website filtering and per-application network allow/block policies | Scoped policy tests cover IPv4/IPv6 as supported; unrelated firewall rules are preserved and owned-rule cleanup verified; coverage, conflicts, and bypass limitations documented. |
| 7 | Crash, restart, reboot, sleep, tamper, upgrade, and database recovery | Recovery reconciles deadline/clock anomalies; expired sessions clean up safely; compromised enforcement is visible; corrupt data and interrupted upgrade paths recover safely. |
| 8 | Secondary Tauri widget and interface polish | Resizing/position/theme preferences and profile visibility/order persist; widget and dashboard show the same authoritative state; closing either leaves service state intact. |
| 9 | Tasks, projects, history, statistics, profile import/export, backup/restore | Task outcome, session completion, and enforcement status remain distinct; imports are validated and cannot start/apply privileged policy; backup restore and corruption recovery are documented and exercised. |
| 10 | Stabilization and native release | Repeatable build/install/upgrade/removal instructions; critical isolated integration tests and recovery steps verified; limitations disclosed. |
| 11 | Separate web dashboard (only after native stable and specifically approved) | Access model, authentication, API, sync/conflict/offline behavior reviewed; no direct DB access or privileged-command bypass. |
| 12 | Mobile, sync, and other expansions (separately approved) | Each expansion has its own requirements and security review. |

## Phase 4A/4B status: hardened simulated lifecycle only

Phase 4A/4B implements a testable policy plan and request-idempotent
apply/verify/rollback journal inside the existing unprivileged Rust/Tauri
application. It derives bounded descriptors from an immutable active-session
snapshot and stores fake resources in separate SQLite tables (migration
`0004`); migration `0005` adds a durable request journal and SQL transition
guards. A UI simulation panel calls the single `simulate_session_policy`
command, exposed by the generated Tauri command ACL. Request IDs are globally
unique in the local database, payload-bound, and replay their original result;
stale operation/session revisions are rejected. Apply/state/event/response
updates are transactional, while interrupted legacy apply/rollback records
can be recovered to their prior simulated resource map.
The application remains `NOT ENFORCING`: it makes no Windows policy or
firewall calls, creates no service or broker, and does not change privileges
or security settings. This phase does not complete the Phase 4 roadmap row
above or authorize Phase 5 enforcement work. The command ACL has been
generated and inspected, but actual WebView IPC has not been verified in a
launched native window. See
[`phase-4a-simulation-design.md`](phase-4a-simulation-design.md).

## First implementation milestone

**Milestone 1: establish a buildable native shell** after the Windows Rust and
Tauri prerequisites are available. It includes branded window metadata,
React/TypeScript/Tailwind dashboard shell, light/dark/system theme foundation,
Rust/Tauri command boundary with no privileged commands, initial SQLite
migration for settings and profiles, and reproducible development/build notes.

The Phase 1 dependency choices must remain on supported stable releases and be
recorded in manifests and lockfiles. Use a documented SQLite library and
migration approach; do not add or install the privileged enforcement component
in Phase 1.

Acceptance: a clean checkout can install documented dependencies and build/run
the shell; the UI cannot claim enforcement; no service, firewall, or other
security-sensitive Windows state is changed; database migrations can be
applied; formatting/type/lint checks and the small baseline test suite run and
their actual results are recorded.

## Verification, documentation, and definition of done

Testing is phased and must report only checks actually run. Phase 1 covers a
clean Windows build, frontend formatting/type/lint checks, Rust formatting and
unit checks, and SQLite migration/transaction checks. Later phases add domain
boundary/state-transition tests, import validation, crash/restart/reboot/sleep
recovery tests, and isolated Windows integration tests for application,
website, and network behavior, firewall preservation, IPv4/IPv6 where
supported, service lifecycle, partial failure, and emergency cleanup. Never
run destructive integration tests against an everyday Windows configuration
without explicit approval.

Maintain a user-facing README, architecture and threat model, development and
debugging guide, isolated test procedures, recovery instructions, and a
changelog. Keep product claims synchronized with verified behavior. A feature
is done only when its correct layer owns it, validation/error handling and
failure display exist, relevant tests actually pass, security implications
and recovery are reviewed, documentation is current, and the UI distinguishes
simulated, applied, and integration-verified enforcement. No UI lock indicator
may imply verified enforcement when it failed or remains unverified.

## Future web dashboard direction

Keep domain rules and serialized contracts presentation-independent where
useful. The web dashboard is a separate app, not a replacement for the native
application. First choose local-only versus remote/multi-device access. Local
access still needs authentication, authorization, origin/request validation,
and a narrowly scoped API; remote access additionally needs an explicit
identity, synchronization, conflict, privacy, offline, and backend design.
Do not introduce a cloud backend or expose service operations through ordinary
HTTP as a shortcut for the initial release.
