# FOCUS --- NATIVE DESKTOP FOCUS & DISTRACTION-CONTROL APP

A minimalist, local-first Windows desktop application for configurable
focus sessions, reusable session profiles, and system-level distraction
controls.

## PRODUCT GOAL

Build a native application that helps the user commit to a chosen
activity and have the computer enforce the restrictions configured for
that session. The application is more than a timer or a website blocker:
it is intended to control selected applications, websites, and network
access during a focus session.

The interface should be minimalistic and support both light and dark
themes.

## SESSION DASHBOARD AND REUSABLE PROFILES

-   The dashboard contains a limited number of visible session slots for
    activities such as Revise, Code, and other user-defined activities.
-   Each session is configurable for its own activity and requirements.
-   Sessions are reusable: configure a session once, save it as a
    profile, and launch it again without recreating its settings.
-   The user can choose which saved sessions are shown in the dashboard
    and desktop widget.
-   Saved profiles can be duplicated to create a variation without
    rebuilding the configuration.
-   A profile can be edited while it is inactive.
-   Profile changes create a proposed new version rather than silently
    overwriting the version already in use.
-   The user must explicitly approve profile changes and choose whether
    they should apply to the current session or the next session.
-   The app presents a comparison of proposed changes before approval.
-   A change intended for the next session leaves the active session
    unchanged.
-   An active session keeps the configuration it started with unless the
    user explicitly approves a permitted update to the current session.
-   A single active strict session should be enforced at a time to avoid
    conflicting sets of restrictions.

## PER-SESSION CONFIGURATION

Every session profile can define the following.

Application access policy:

-   Whitelist: only explicitly approved applications are permitted,
    subject to essential Windows processes and enforcement safeguards.
-   Blacklist: selected applications are blocked while other
    applications remain available.
-   The configuration should allow the user to choose the applications
    or executable programs to permit or block. Restrictions should cover
    attempts to launch blocked applications and, where needed, detect
    and handle attempts to reopen them. Selected background processes
    and startup applications may also be restricted where supported.
-   Essential Windows components and access required to operate or
    recover the computer must not be accidentally blocked by a strict
    whitelist.

Website restrictions:

-   Configure website restrictions for each session.
-   Apply website restrictions beyond a single browser where supported
    by the operating system and filtering approach.
-   Account for common bypass attempts such as switching browsers or
    changing DNS configuration.
-   Website filtering may require DNS filtering and additional controls;
    VPNs, encrypted DNS, shared hosting, and other network arrangements
    can limit the guarantees of domain blocking.

Network restrictions:

-   Configure network access for selected applications or programs.
-   Block a selected application's internet access while allowing other
    applications to remain online.
-   Where supported, block all network traffic for a selected executable
    or allow only selected destinations.
-   Where supported, distinguish local-network access from internet
    access.
-   Apply network restrictions during the configured focus session.
-   Preserve unrelated existing Windows Firewall rules when applying or
    removing the app's rules.

Timer and completion policy:

-   Timer: the session completes automatically when its timer expires.
-   Timer + task confirmation: after the timer expires, the user must
    confirm whether the planned task was completed.
-   Each profile also defines its session duration and any configured
    timer settings. A planned break may be included in a profile with
    its duration and permitted applications configured in advance; a
    break must not act as an unplanned way to escape a locked session.

## ACTIVE STRICT-SESSION BEHAVIOUR

-   Before a session starts, show the user its duration, task or
    activity, permitted applications, blocked applications, website
    rules, network rules, and completion policy.
-   Validate that the configured policy can be enforced before starting.
    If a critical restriction cannot be applied, do not claim the
    session is fully enforced.
-   After the user confirms the commitment, apply the configured
    restrictions and lock the session configuration.
-   During a strict session, the only restriction setting the user may
    alter is the remaining time, within the time-adjustment rules below.
-   Application, website, network, and other enforcement rules remain
    locked during the session, except for an explicitly approved profile
    update that is permitted by the session's change policy.
-   Closing the dashboard must not cancel the session or release
    restrictions.
-   The enforcement service must maintain restrictions independently of
    the dashboard.
-   Detect attempts to terminate the dashboard or service, alter
    firewall rules, launch blocked applications, or otherwise tamper
    with enforcement. Recover restrictions where possible and clearly
    report when enforcement is compromised.
-   The app must not claim to be unbreakable. A user with unrestricted
    administrator access may still be able to bypass restrictions unless
    stronger operating-system-managed policies are configured.
-   Provide a deliberate, protected recovery mechanism for emergencies,
    broken policies, crashes, and technical failures. Recovery must not
    be an ordinary easy-cancellation control.

## TIME ADJUSTMENT DURING A STRICT SESSION

Time is the only restriction the user can adjust during an active strict
session. Shortening the session is bounded by the time remaining at the
moment of adjustment:

-   If more than 60 minutes remain, the user cannot reduce the remaining
    time by more than half.
-   If 30 to 60 minutes remain, the remaining time cannot be reduced
    below 30 minutes.
-   If more than 15 minutes but less than 30 minutes remain, the
    remaining time cannot be reduced below 15 minutes.
-   If 15 minutes or less remain, the session cannot be shortened
    further.

Examples:

Time remaining before adjustment \| Minimum remaining time afterward 120
minutes \| 60 minutes 90 minutes \| 45 minutes 60 minutes \| 30 minutes
45 minutes \| 30 minutes 30 minutes \| 30 minutes 20 minutes \| 15
minutes 15 minutes \| 15 minutes

The enforcement service---not only the dashboard---must validate time
adjustments so the limit cannot be bypassed by manipulating the UI.

Session extensions are separate from shortening. An extension should
require explicit confirmation, and the updated deadline must be recorded
by the enforcement service.

## TIMER PERSISTENCE, SLEEP, AND RESTART RECOVERY

-   Save active session state independently of the UI.
-   If the dashboard or application process closes or crashes, the
    restrictions remain active.
-   If Windows restarts, retain the active session record and restore
    restrictions where possible.
-   Calculate remaining time from a reliable deadline so a restart does
    not accidentally restart the full duration.
-   Use elapsed wall-clock time for the timer. If the computer sleeps
    through the deadline, reconcile the session and safely release
    restrictions on wake if the session has expired.
-   If enforcement cannot be restored, clearly report the issue rather
    than falsely presenting the session as protected.

## DESKTOP QUICK-LAUNCH WIDGET

Provide a small native desktop widget or secondary window that lets the
user launch saved focus sessions without opening the full dashboard.

-   The widget is resizable, including through its edges or corners.
-   Support compact and larger layouts: a compact layout shows favourite
    sessions; a larger layout can show more session details and
    shortcuts.
-   Include an optional always-on-top mode.
-   Display the user's chosen sessions in their chosen order, subject to
    the limited visible slots.
-   A session click opens a commitment/confirmation view showing the
    session's configuration; clicking a widget item alone must not
    accidentally start a locked session.
-   During an active session, show the activity, live countdown, and
    lock status.
-   The widget must reflect the enforcement service's actual session
    state rather than run an independent timer.
-   Remember the widget's size and position across restarts.
-   Support light and dark appearance, with an option to follow the
    system theme.
-   The intended implementation is a secondary Tauri window connected to
    the same Rust backend as the main application.

## USER INTERFACE AND APPEARANCE

-   Minimalistic desktop interface with both light and dark themes.
-   Provide a manual theme choice and an optional system-theme setting.
-   Use consistent typography, spacing, icons, controls, and interaction
    patterns across themes.
-   Keep the dashboard focused on session slots, session status, and a
    clear start action.
-   Keep the active-session screen quiet and focused on the activity,
    timer, and lock status.
-   Use subtle visual states for ready, active, locked, and completed
    sessions.
-   Avoid distracting gamification, unnecessary charts, excessive
    decoration, and pop-ups that interrupt focus.
-   Provide quiet notifications for session start, halfway point, and
    completion.

## TASKS, PROJECTS, HISTORY, AND STATISTICS

-   Allow sessions to link to tasks and projects. For example, a coding
    session can be linked to a software project, and a revision session
    can be linked to a course unit.
-   Record the planned task or intended outcome, not only the time
    spent.
-   Keep a session history and show a completion summary.
-   Track completed sessions, planned versus actual focus time, tasks
    finished, and consistency over time.
-   Statistics should support progress without rewarding meaningless
    screen time.
-   Show enforcement issues in the session summary where applicable.

## PRE-SESSION CHECKS AND DIAGNOSTICS

Before starting a strict session:

-   Verify that necessary applications are available.
-   Verify that the configured restrictions can be enforced.
-   Check the timer and completion policy.
-   Preserve essential Windows access and recovery capability.
-   Clearly explain the applications, websites, and network access
    affected by the profile.
-   Include a test mode to validate a profile before relying on it.
-   If a critical restriction fails, do not represent the session as
    fully enforced.

## LOCAL DATA AND RECOVERY

-   Store profiles, settings, tasks/projects, and session history
    locally in SQLite.
-   Provide export and restore options for settings, profiles, and
    history.
-   Protect against losing configurations due to a database problem or
    application update.
-   Importable/exportable profiles can be used to move configurations
    between the user's devices.

## TECHNICAL DIRECTION

The intended first version is Windows-first and local-first.

Component \| Intended role React + TypeScript \| Dashboard and interface
Tailwind CSS \| Interface styling Tauri \| Native desktop application
and secondary widget window Rust \| Native application logic and
controlled communication with system functions Privileged Windows
background service \| Enforce restrictions independently of the
dashboard SQLite \| Local profiles, settings, tasks/projects, and
session history Windows Firewall \| Executable-specific network rules
and network restrictions Windows Filtering Platform (WFP) \| Deeper
network filtering if needed

The enforcement layer should use supported Windows security interfaces
where possible. More advanced enforcement may require Windows Filtering
Platform components. Running as administrator alone does not guarantee
every restriction.

A separate cloud backend is not required for version one. A future
mobile companion may be considered later, while Windows enforcement
remains native to Windows.

## SUGGESTED IMPLEMENTATION ORDER

1.  Establish the native Tauri/Rust foundation and the Windows service
    with a clear permission model.
2.  Implement application selection and application restrictions.
3.  Implement per-application network restrictions and safe rule
    restoration.
4.  Implement website filtering and test common bypass paths.
5.  Implement reusable profiles, version proposals, explicit approvals,
    and current-session versus next-session application.
6.  Implement locked sessions, bounded time adjustments, timer/task
    completion, and restart recovery.
7.  Implement the resizable desktop widget, light/dark themes, local
    storage, tasks/projects, history, and statistics.
8.  Test enforcement and recovery when the UI closes, processes restart,
    Windows reboots, the computer sleeps, a policy is tampered with, or
    a configured restriction fails.

## SCOPE DISCIPLINE

The first release should focus on:

1.  Minimal light/dark dashboard with limited visible session slots.
2.  Reusable, versioned profiles with per-session whitelist or blacklist
    application policies.
3.  Per-session timer-only or timer-plus-task completion.
4.  Native Windows application restrictions, per-application network
    restrictions, and website filtering.
5.  Privileged background enforcement, persistence, tamper detection,
    and recovery.
6.  A resizable desktop quick-launch widget.
7.  Local SQLite storage, task/project linking, session history, and
    basic statistics.

Adaptive planning, richer focus reports, and a mobile companion are
later possibilities. AI-generated plans, social leaderboards, and
elaborate gamification are not part of the initial release.

## IMPORTANT LIMITATIONS

The application should aim for meaningful resistance to ordinary bypass
attempts, but must not promise an unbreakable lock. A user with
unrestricted administrator access may still be able to disable or bypass
controls unless stronger OS-managed policies are configured. Website
blocking can also be affected by VPNs, encrypted DNS, shared hosting,
and other network arrangements.

Safety and recovery are part of the design: do not block essential
Windows components, preserve unrelated firewall rules, validate policies
before locking, and provide a protected recovery path for emergencies
and technical failures.
