import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { BrandLogo } from "./components/BrandLogo";
import {
  emptyConfig,
  validateProfile,
  type NamedItem,
  type Profile,
  type ProfileConfig,
  type SessionStatus,
  type SessionView,
  type SimulationOperation,
} from "./domain";
import {
  applyTheme,
  readThemePreference,
  saveThemePreference,
  type ThemePreference,
} from "./theme";

type Page = "Overview" | "Profiles" | "Tasks" | "Projects";
const themeLabels: Record<ThemePreference, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};
const SIMULATION_OPERATION_KEY = "focusmimi.simulation.operation";
function newRequestId() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return `ui-${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function App() {
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);
  const [page, setPage] = useState<Page>("Overview");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tasks, setTasks] = useState<NamedItem[]>([]);
  const [projects, setProjects] = useState<NamedItem[]>([]);
  const [error, setError] = useState("");
  const [editor, setEditor] = useState<{
    profile: Profile | null;
    version: Profile["versions"][number] | null;
  } | null>(null);
  const [itemName, setItemName] = useState("");
  const [sessionStatus, setSessionStatus] = useState<SessionStatus>({
    active: null,
    awaitingCompletion: [],
  });
  const [review, setReview] = useState<{
    profile: Profile;
    version: Profile["versions"][number];
  } | null>(null);
  const [confirmation, setConfirmation] = useState(false);
  const [shortenMinutes, setShortenMinutes] = useState(30);
  const [simulationOperation, setSimulationOperation] = useState<SimulationOperation | null>(null);
  const [simulationBusy, setSimulationBusy] = useState(false);
  const [simulationError, setSimulationError] = useState("");
  const simulationLock = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [p, t, j] = await Promise.all([api.profiles(), api.tasks(), api.projects()]);
      setProfiles(p);
      setTasks(t);
      setProjects(j);
    } catch {
      setError(
        "The local workspace could not be loaded. Run FOCUS by MIMI as a native desktop app to use SQLite storage.",
      );
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const refreshSession = useCallback(async () => {
    try {
      setSessionStatus(await api.sessionStatus());
    } catch (e) {
      setError(`Session status could not be loaded: ${String(e)}`);
    }
  }, []);
  useEffect(() => {
    void refreshSession();
    const timer = window.setInterval(() => void refreshSession(), 1000);
    return () => window.clearInterval(timer);
  }, [refreshSession]);
  useEffect(() => {
    const operationId = window.localStorage.getItem(SIMULATION_OPERATION_KEY);
    if (!operationId) {
      setSimulationOperation(null);
      return;
    }
    void api
      .simulatePolicy({ action: "status", operationId })
      .then((operation) => {
        if (operation.plan.sessionId === sessionStatus.active?.id) {
          setSimulationOperation(operation);
        } else {
          window.localStorage.removeItem(SIMULATION_OPERATION_KEY);
          setSimulationOperation(null);
        }
      })
      .catch((e) => {
        setSimulationError(`Saved simulation status could not be loaded: ${String(e)}`);
      });
  }, [sessionStatus.active?.id]);
  async function performSession(action: () => Promise<SessionView>) {
    try {
      await action();
      await refreshSession();
      setError("");
      return true;
    } catch (e) {
      setError(String(e));
      await refreshSession();
      return false;
    }
  }
  async function runPolicySimulation() {
    if (!activeSession || simulationLock.current) return;
    simulationLock.current = true;
    setSimulationBusy(true);
    setSimulationError("");
    try {
      const prepared = await api.simulatePolicy({
        action: "prepare",
        sessionId: activeSession.id,
        expectedProfileVersion: activeSession.profileVersion,
        expectedRevision: activeSession.revision,
        requestId: newRequestId(),
      });
      setSimulationOperation(prepared);
      window.localStorage.setItem(SIMULATION_OPERATION_KEY, prepared.operationId);
      const applied = await api.simulatePolicy({
        action: "apply",
        operationId: prepared.operationId,
        requestId: newRequestId(),
        expectedOperationRevision: prepared.revision,
        expectedSessionRevision: activeSession.revision,
      });
      setSimulationOperation(applied);
      const verified = await api.simulatePolicy({
        action: "verify",
        operationId: applied.operationId,
        requestId: newRequestId(),
        expectedOperationRevision: applied.revision,
        expectedSessionRevision: activeSession.revision,
      });
      setSimulationOperation(verified);
    } catch (e) {
      setSimulationError(`Policy simulation was rejected or interrupted: ${String(e)}`);
      await refreshSession();
    } finally {
      simulationLock.current = false;
      setSimulationBusy(false);
    }
  }
  async function rollbackPolicySimulation() {
    if (!simulationOperation || simulationLock.current) return;
    simulationLock.current = true;
    setSimulationBusy(true);
    setSimulationError("");
    try {
      const rolledBack = await api.simulatePolicy({
        action: "rollback",
        operationId: simulationOperation.operationId,
        requestId: newRequestId(),
        expectedOperationRevision: simulationOperation.revision,
      });
      setSimulationOperation(rolledBack);
    } catch (e) {
      setSimulationError(`Simulation rollback failed: ${String(e)}`);
    } finally {
      simulationLock.current = false;
      setSimulationBusy(false);
    }
  }
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => applyTheme("system");
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [theme]);
  function changeTheme(next: ThemePreference) {
    saveThemePreference(next);
    setTheme(next);
  }

  async function saveProfile(name: string, description: string, config: ProfileConfig) {
    const validation = validateProfile(name, description, config);
    if (validation) {
      setError(validation);
      return;
    }
    try {
      await api.saveProfile(editor?.profile?.id ?? null, name, description, config);
      setEditor(null);
      setError(
        "Saved as a proposal. It will not change the approved profile until reviewed and approved.",
      );
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function approve(profile: Profile, version: number) {
    try {
      await api.approve(profile.id, version);
      setError("Approved version updated. Existing versions remain in history.");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function removeProfile(profile: Profile) {
    if (!window.confirm(`Delete ${profile.name} and its saved versions?`)) return;
    try {
      await api.deleteProfile(profile.id);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function changeVisibility(profile: Profile, visible: boolean) {
    try {
      await api.setProfileVisible(profile.id, visible);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function moveProfile(profile: Profile, delta: -1 | 1) {
    const ordered = [...profiles].sort((a, b) => a.sortOrder - b.sortOrder);
    const index = ordered.findIndex((item) => item.id === profile.id);
    const target = index + delta;
    if (target < 0 || target >= ordered.length) return;
    [ordered[index], ordered[target]] = [ordered[target], ordered[index]];
    try {
      await api.setProfileOrder(ordered.map((item) => item.id));
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function addItem() {
    if (!itemName.trim()) {
      setError("Enter a name first.");
      return;
    }
    try {
      if (page === "Projects") await api.saveProject(null, itemName.trim(), "");
      else await api.saveTask(null, itemName.trim(), null);
      setItemName("");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  const nav: Page[] = ["Overview", "Profiles", "Tasks", "Projects"];
  const activeSession = sessionStatus.active;
  const displayedProfiles =
    page === "Overview" ? profiles.filter((profile) => profile.visible) : profiles;
  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="Main navigation">
        <div className="brand">
          <BrandLogo />
        </div>
        <div className="sidebar-label">WORKSPACE</div>
        <nav className="nav-list">
          {nav.map((item) => (
            <button
              className={`nav-item${page === item ? " active" : ""}`}
              type="button"
              key={item}
              onClick={() => {
                setPage(item);
                setError("");
              }}
              aria-current={page === item ? "page" : undefined}
            >
              {item}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">Local-first · SQLite</div>
      </aside>
      <main className="main-content">
        <header className="topbar">
          <div className="header-context">
            <BrandLogo compact />
            <div className="breadcrumb">
              Workspace <span>/</span> {page}
            </div>
          </div>
          <label className="theme-control">
            <span>Appearance</span>
            <select
              aria-label="Appearance"
              value={theme}
              onChange={(e) => changeTheme(e.target.value as ThemePreference)}
            >
              {(Object.keys(themeLabels) as ThemePreference[]).map((x) => (
                <option key={x} value={x}>
                  {themeLabels[x]}
                </option>
              ))}
            </select>
          </label>
        </header>
        <div className="page-content">
          <section className="welcome-row">
            <div>
              <p className="eyebrow">FOCUS BY MIMI</p>
              <h1>{page === "Overview" ? "Make room for what matters." : page}</h1>
              <p className="welcome-copy">Commit. Focus. Complete.</p>
            </div>
          </section>
          <section className="status-banner" aria-label="Enforcement status">
            <div className="status-symbol" aria-hidden="true">
              i
            </div>
            <div className="status-copy">
              <strong>Session timer is available · restrictions are not active</strong>
              <span>
                This local timer does not enforce application, website, or network policies.
              </span>
            </div>
            <span className="status-badge">NOT ENFORCING</span>
          </section>
          {error && (
            <div role="status" className="notice">
              {error}
            </div>
          )}
          <section className="panel session-panel" aria-label="Session timer">
            {activeSession ? (
              <>
                <p className="eyebrow">CURRENT SESSION · NOT ENFORCING</p>
                <h2>
                  {activeSession.snapshot.activity} ·{" "}
                  {activeSession.state === "active_break" ? "Break" : "Focus"}
                </h2>
                <p>{activeSession.snapshot.intendedOutcome}</p>
                <strong>{formatRemaining(activeSession.remainingMs)}</strong>
                <p className="muted">
                  Approved profile v{activeSession.profileVersion}; saved deadline remains
                  authoritative in the Rust app.
                </p>
                <section className="simulation-panel" aria-label="Policy simulation">
                  <p className="eyebrow">SIMULATION ONLY · NO RESTRICTIONS ARE APPLIED</p>
                  <p className="muted">
                    This previews the saved session policy in local simulation data. Apps, websites,
                    and networks remain unrestricted.
                  </p>
                  {simulationOperation?.plan.sessionId === activeSession.id ? (
                    <div aria-live="polite">
                      <p>
                        Simulation state:{" "}
                        <strong>{simulationOperation.state.replaceAll("_", " ")}</strong>
                      </p>
                      {simulationOperation.error && (
                        <p role="status">{simulationOperation.error}</p>
                      )}
                      {simulationOperation.state !== "rolled_back" &&
                        simulationOperation.state !== "failed" && (
                          <button
                            className="button-secondary"
                            type="button"
                            disabled={simulationBusy}
                            onClick={() => void rollbackPolicySimulation()}
                          >
                            Roll back simulation
                          </button>
                        )}
                      {simulationOperation.state === "rolled_back" && (
                        <button
                          className="button-secondary"
                          type="button"
                          disabled={simulationBusy}
                          onClick={() => void runPolicySimulation()}
                        >
                          Run another policy simulation
                        </button>
                      )}
                    </div>
                  ) : (
                    <button
                      className="button-secondary"
                      type="button"
                      disabled={simulationBusy}
                      onClick={() => void runPolicySimulation()}
                    >
                      {simulationBusy ? "Running simulation…" : "Run policy simulation"}
                    </button>
                  )}
                  {simulationError && <p role="alert">{simulationError}</p>}
                </section>
                <div className="card-actions">
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      void performSession(() =>
                        api.shortenSession(
                          activeSession.id,
                          activeSession.revision,
                          shortenMinutes,
                        ),
                      );
                    }}
                  >
                    <label>
                      Shorten to remaining minutes{" "}
                      <input
                        type="number"
                        min={1}
                        value={shortenMinutes}
                        onChange={(e) => setShortenMinutes(Number(e.target.value))}
                      />
                    </label>
                    <button className="button-secondary" type="submit">
                      Apply shortening
                    </button>
                  </form>
                  {activeSession.state === "active_work" &&
                    activeSession.breakIndex < activeSession.snapshot.plannedBreaks.length && (
                      <button
                        className="button-secondary"
                        type="button"
                        onClick={() =>
                          void performSession(() =>
                            api.startBreak(activeSession.id, activeSession.revision),
                          )
                        }
                      >
                        Start next planned break
                      </button>
                    )}
                  {activeSession.state === "active_break" && (
                    <button
                      className="button-secondary"
                      type="button"
                      onClick={() =>
                        void performSession(() =>
                          api.skipBreak(activeSession.id, activeSession.revision),
                        )
                      }
                    >
                      End break
                    </button>
                  )}
                </div>
              </>
            ) : (
              <>
                <p className="eyebrow">SESSION TIMER</p>
                <h2>No active session</h2>
                <p>
                  Choose an approved, visible profile to review its complete saved settings before
                  confirming.
                </p>
              </>
            )}
            {sessionStatus.awaitingCompletion.map((item) => (
              <article key={item.id} className="completion-prompt">
                <h3>Session ended · {item.snapshot.activity}</h3>
                <p>{item.snapshot.intendedOutcome}</p>
                <p>Choose an outcome to complete this record. No answer is assumed.</p>
                {(["completed", "partial", "not_completed"] as const).map((outcome) => (
                  <button
                    className="button-secondary"
                    key={outcome}
                    type="button"
                    onClick={() =>
                      void performSession(() => api.recordOutcome(item.id, item.revision, outcome))
                    }
                  >
                    {outcome.replaceAll("_", " ")}
                  </button>
                ))}
              </article>
            ))}
          </section>
          {(page === "Overview" || page === "Profiles") && (
            <section className="panel profile-panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">YOUR ROUTINES</p>
                  <h2>Session profiles</h2>
                </div>
                <button
                  className="button-primary"
                  type="button"
                  onClick={() => setEditor({ profile: null, version: null })}
                >
                  New profile
                </button>
              </div>
              {displayedProfiles.length === 0 ? (
                <div className="empty-state">
                  <BrandLogo compact />
                  <h3>Create your first profile</h3>
                  <p>
                    Save reusable activity and policy settings. Saving creates a proposal for
                    review.
                  </p>
                  <button
                    className="button-secondary"
                    type="button"
                    onClick={() => setEditor({ profile: null, version: null })}
                  >
                    Create profile
                  </button>
                </div>
              ) : (
                <div className="profile-grid">
                  {displayedProfiles.map((profile) => {
                    const approved = profile.versions.find(
                      (v) => v.version === profile.approvedVersion,
                    );
                    const proposed = profile.versions.find((v) => v.status === "proposed");
                    return (
                      <article className="profile-card" key={profile.id}>
                        <p className="eyebrow">{approved?.config.activity ?? "PROFILE"}</p>
                        <h3>{profile.name}</h3>
                        <p>
                          {profile.description ||
                            approved?.config.intendedOutcome ||
                            "No description"}
                        </p>
                        <p className="muted">
                          {approved ? `Approved v${approved.version}` : "No approved version"}
                          {proposed ? ` · Proposal v${proposed.version}` : ""}
                        </p>
                        <label className="visibility-toggle">
                          <input
                            type="checkbox"
                            checked={profile.visible}
                            onChange={(e) => void changeVisibility(profile, e.target.checked)}
                          />
                          Show on dashboard
                        </label>
                        <p className="muted">
                          Version history:{" "}
                          {profile.versions.map((v) => `v${v.version} ${v.status}`).join(" · ")}
                        </p>
                        {approved && proposed && (
                          <ProfileComparison
                            approved={approved.config}
                            proposed={proposed.config}
                          />
                        )}
                        {approved && profile.visible && (
                          <button
                            className="button-primary"
                            type="button"
                            disabled={!!sessionStatus.active}
                            onClick={() => {
                              setReview({ profile, version: approved });
                              setConfirmation(false);
                            }}
                          >
                            Review & start session
                          </button>
                        )}
                        <div className="card-actions">
                          <button
                            className="button-secondary"
                            type="button"
                            aria-label={`Move ${profile.name} up`}
                            onClick={() => void moveProfile(profile, -1)}
                          >
                            ↑
                          </button>
                          <button
                            className="button-secondary"
                            type="button"
                            aria-label={`Move ${profile.name} down`}
                            onClick={() => void moveProfile(profile, 1)}
                          >
                            ↓
                          </button>
                          <button
                            className="button-secondary"
                            onClick={() =>
                              setEditor({ profile, version: proposed ?? approved ?? null })
                            }
                            type="button"
                          >
                            Edit proposal
                          </button>
                          <button
                            className="button-secondary"
                            onClick={() =>
                              setEditor({ profile: null, version: approved ?? proposed ?? null })
                            }
                            type="button"
                          >
                            Duplicate
                          </button>
                          <button
                            className="button-secondary"
                            type="button"
                            onClick={() => void removeProfile(profile)}
                          >
                            Delete
                          </button>
                        </div>
                      </article>
                    );
                  })}
                </div>
              )}
            </section>
          )}
          {(page === "Tasks" || page === "Projects") && (
            <section className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">WORKSPACE</p>
                  <h2>{page}</h2>
                </div>
              </div>
              <form
                className="inline-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  void addItem();
                }}
              >
                <input
                  aria-label={`${page} name`}
                  value={itemName}
                  onChange={(e) => setItemName(e.target.value)}
                  placeholder={`New ${page.slice(0, -1).toLowerCase()} name`}
                  maxLength={160}
                />
                <button className="button-primary" type="submit">
                  Add {page.slice(0, -1).toLowerCase()}
                </button>
              </form>
              <ul className="item-list">
                {(page === "Tasks" ? tasks : projects).map((item) => (
                  <li key={item.id}>
                    {item.name}
                    {item.completed ? " · Complete" : ""}
                  </li>
                ))}
              </ul>
              {(page === "Tasks" ? tasks : projects).length === 0 && (
                <p className="muted">No {page.toLowerCase()} yet.</p>
              )}
            </section>
          )}
          {page === "Overview" && (
            <section className="overview-grid">
              <article className="panel">
                <div className="panel-heading">
                  <div>
                    <p className="eyebrow">TASKS</p>
                    <h2>Tasks</h2>
                  </div>
                  <span className="count-pill">{tasks.length}</span>
                </div>
                <p>Plan work alongside reusable profiles.</p>
                <button className="button-secondary" onClick={() => setPage("Tasks")} type="button">
                  Manage tasks
                </button>
              </article>
              <article className="panel">
                <div className="panel-heading">
                  <div>
                    <p className="eyebrow">PROJECTS</p>
                    <h2>Projects</h2>
                  </div>
                  <span className="count-pill">{projects.length}</span>
                </div>
                <p>Group related tasks into projects.</p>
                <button
                  className="button-secondary"
                  onClick={() => setPage("Projects")}
                  type="button"
                >
                  Manage projects
                </button>
              </article>
            </section>
          )}
        </div>
      </main>
      {editor && (
        <ProfileEditor
          key={`${editor.profile?.id ?? "new"}-${editor.version?.version ?? "none"}`}
          profile={editor.profile}
          version={editor.version}
          tasks={tasks}
          projects={projects}
          onClose={() => setEditor(null)}
          onSave={saveProfile}
          onApprove={async (target, version) => {
            await approve(target, version);
            setEditor(null);
          }}
        />
      )}
      {review && (
        <div className="modal-backdrop">
          <section
            className="editor-modal session-review"
            role="dialog"
            aria-modal="true"
            aria-label="Review session profile"
          >
            <h2>Review approved profile v{review.version.version}</h2>
            <h3>
              {review.profile.name} · {review.version.config.activity}
            </h3>
            <p>{review.version.config.intendedOutcome}</p>
            <p>Duration: {review.version.config.durationMinutes} minutes</p>
            <p>
              Project:{" "}
              {projects.find((x) => x.id === review.version.config.projectId)?.name ?? "None"}
              {"; task: "}
              {tasks.find((x) => x.id === review.version.config.taskId)?.name ?? "None"}
            </p>
            <PolicySummary
              title="Application policy"
              mode={review.version.config.applicationPolicy.mode}
              entries={review.version.config.applicationPolicy.entries}
            />
            <PolicySummary
              title="Website policy"
              mode={review.version.config.websitePolicy.mode}
              entries={review.version.config.websitePolicy.domains}
            />
            <PolicySummary
              title="Per-application network policy"
              mode={review.version.config.networkPolicy.mode}
              entries={review.version.config.networkPolicy.applications}
            />
            <p>
              Completion: {review.version.config.completionPolicy.replaceAll("_", " ")}. Planned
              breaks:{" "}
              {review.version.config.plannedBreaks
                .map(
                  (b) =>
                    `${b.durationMinutes} min (allowed applications: ${b.allowedApplications.join(", ") || "none"})`,
                )
                .join(", ") || "None"}
              .
            </p>
            <p className="muted">Timer only. No restrictions are applied in this phase.</p>
            <label className="review-confirmation">
              <input
                type="checkbox"
                checked={confirmation}
                onChange={(e) => setConfirmation(e.target.checked)}
              />{" "}
              I reviewed these settings and explicitly want to start this session.
            </label>
            <div className="card-actions">
              <button
                className="button-primary"
                type="button"
                disabled={!confirmation || !!sessionStatus.active}
                onClick={() =>
                  void performSession(() =>
                    api.startSession(review.profile.id, review.version.version, true),
                  ).then((success) => {
                    if (success) setReview(null);
                  })
                }
              >
                Confirm and start
              </button>
              <button className="button-secondary" type="button" onClick={() => setReview(null)}>
                Cancel
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

function formatRemaining(ms: number) {
  const seconds = Math.ceil(ms / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
function PolicySummary({
  title,
  mode,
  entries,
}: {
  title: string;
  mode: string;
  entries: string[];
}) {
  return (
    <p>
      <strong>{title}:</strong> {mode.replaceAll("_", " ")} ·{" "}
      {entries.length ? entries.join(", ") : "no entries"}
    </p>
  );
}

function ProfileComparison({
  approved,
  proposed,
  expanded = false,
}: {
  approved: Profile["versions"][number]["config"];
  proposed: Profile["versions"][number]["config"];
  expanded?: boolean;
}) {
  const approvedValues = approved as unknown as Record<string, unknown>;
  const proposedValues = proposed as unknown as Record<string, unknown>;
  const keys = [
    ...new Set([...Object.keys(approvedValues), ...Object.keys(proposedValues)]),
  ].filter((key) => JSON.stringify(approvedValues[key]) !== JSON.stringify(proposedValues[key]));
  if (!keys.length)
    return <p className="muted">The proposed configuration matches the approved version.</p>;
  return (
    <details className="version-comparison" open={expanded}>
      <summary>
        Review {keys.length} changed field{keys.length === 1 ? "" : "s"}
      </summary>
      <ul>
        {keys.map((key) => (
          <li key={key}>
            <strong>{key}</strong>: {JSON.stringify(approvedValues[key] ?? null)} →{" "}
            {JSON.stringify(proposedValues[key] ?? null)}
          </li>
        ))}
      </ul>
    </details>
  );
}

function ProfileEditor({
  profile,
  version,
  tasks,
  projects,
  onClose,
  onSave,
  onApprove,
}: {
  profile: Profile | null;
  version: Profile["versions"][number] | null;
  tasks: NamedItem[];
  projects: NamedItem[];
  onClose: () => void;
  onSave: (name: string, description: string, config: ProfileConfig) => Promise<void>;
  onApprove: (profile: Profile, version: number) => Promise<void>;
}) {
  const [name, setName] = useState(profile?.name ?? "");
  const [description, setDescription] = useState(profile?.description ?? "");
  const [config, setConfig] = useState<ProfileConfig>(version?.config ?? emptyConfig());
  const [reviewed, setReviewed] = useState(false);
  const approvedVersion =
    profile?.versions.find((item) => item.version === profile.approvedVersion) ?? null;
  function field<K extends keyof ProfileConfig>(key: K, value: ProfileConfig[K]) {
    setConfig((c) => ({ ...c, [key]: value }));
  }
  return (
    <div className="modal-backdrop">
      <form
        className="editor-modal"
        onSubmit={(e) => {
          e.preventDefault();
          void onSave(name, description, config);
        }}
      >
        <div className="panel-heading">
          <h2>{profile ? `Propose changes to ${profile.name}` : "New profile"}</h2>
          <button className="button-secondary" type="button" onClick={onClose}>
            Close
          </button>
        </div>
        <label>
          Profile name
          <input maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        {profile && version?.status === "proposed" && (
          <section className="approval-review" aria-label="Version approval review">
            <h3>Review proposal v{version.version}</h3>
            {approvedVersion ? (
              <ProfileComparison
                expanded
                approved={approvedVersion.config}
                proposed={version.config}
              />
            ) : (
              <p className="muted">
                This is the first proposed version; it will become the approved baseline.
              </p>
            )}
            <label className="review-confirmation">
              <input
                type="checkbox"
                checked={reviewed}
                onChange={(e) => setReviewed(e.target.checked)}
              />
              I reviewed the proposed changes
            </label>
            <button
              className="button-secondary"
              type="button"
              disabled={!reviewed}
              onClick={() => void onApprove(profile, version.version)}
            >
              Approve version {version.version}
            </button>
          </section>
        )}
        <label>
          Description
          <textarea
            maxLength={1000}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </label>
        <label>
          Activity
          <select value={config.activity} onChange={(e) => field("activity", e.target.value)}>
            <option>Revise</option>
            <option>Code</option>
            <option>Assignments</option>
            <option>Custom</option>
          </select>
        </label>
        <label>
          Intended outcome
          <textarea
            maxLength={500}
            value={config.intendedOutcome}
            onChange={(e) => field("intendedOutcome", e.target.value)}
            placeholder="What will be complete?"
          />
        </label>
        <label>
          Duration (minutes)
          <input
            type="number"
            min={5}
            max={1440}
            value={config.durationMinutes}
            onChange={(e) => field("durationMinutes", Number(e.target.value))}
          />
        </label>
        <label>
          Project
          <select
            value={config.projectId ?? ""}
            onChange={(e) => field("projectId", e.target.value || null)}
          >
            <option value="">No project</option>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Task
          <select
            value={config.taskId ?? ""}
            onChange={(e) => field("taskId", e.target.value || null)}
          >
            <option value="">No task</option>
            {tasks.map((task) => (
              <option key={task.id} value={task.id}>
                {task.name}
              </option>
            ))}
          </select>
        </label>
        <PolicyEditor
          title="Application policy"
          mode={config.applicationPolicy.mode}
          values={config.applicationPolicy.entries}
          onChange={(mode, entries) => field("applicationPolicy", { mode, entries })}
        />
        <PolicyEditor
          title="Website policy"
          mode={config.websitePolicy.mode}
          values={config.websitePolicy.domains}
          onChange={(mode, domains) => field("websitePolicy", { mode, domains })}
        />
        <PolicyEditor
          title="Per-application network policy"
          mode={config.networkPolicy.mode}
          values={config.networkPolicy.applications}
          onChange={(mode, applications) => field("networkPolicy", { mode, applications })}
        />
        <label>
          Completion policy
          <select
            value={config.completionPolicy}
            onChange={(e) =>
              field("completionPolicy", e.target.value as ProfileConfig["completionPolicy"])
            }
          >
            <option value="timer_only">Timer only</option>
            <option value="task_confirmation">Confirm task completion</option>
          </select>
        </label>
        <label>
          Planned breaks (minutes, comma-separated)
          <input
            aria-label="Planned breaks"
            value={config.plannedBreaks.map((item) => item.durationMinutes).join(", ")}
            onChange={(e) =>
              field(
                "plannedBreaks",
                e.target.value
                  .split(",")
                  .map((value) => value.trim())
                  .filter(Boolean)
                  .map((value) => ({ durationMinutes: Number(value), allowedApplications: [] })),
              )
            }
          />
        </label>
        <p className="muted">
          Policy settings are stored for future session support. This application currently does not
          enforce them.
        </p>
        <div className="card-actions">
          <button className="button-primary" type="submit">
            Save proposal
          </button>
          <button className="button-secondary" type="button" onClick={onClose}>
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}

function PolicyEditor({
  title,
  mode,
  values,
  onChange,
}: {
  title: string;
  mode: ProfileConfig["applicationPolicy"]["mode"];
  values: string[];
  onChange: (mode: ProfileConfig["applicationPolicy"]["mode"], values: string[]) => void;
}) {
  return (
    <fieldset>
      <legend>{title}</legend>
      <select value={mode} onChange={(e) => onChange(e.target.value as typeof mode, values)}>
        <option value="disabled">Disabled</option>
        <option value="whitelist">Allow listed</option>
        <option value="blacklist">Block listed</option>
      </select>
      <input
        aria-label={`${title} entries`}
        value={values.join(", ")}
        placeholder="Comma-separated entries"
        onChange={(e) =>
          onChange(
            mode,
            e.target.value
              .split(",")
              .map((x) => x.trim())
              .filter(Boolean),
          )
        }
      />
    </fieldset>
  );
}

export default App;
