export type PolicyMode = "disabled" | "whitelist" | "blacklist";
export type ProfileConfig = {
  activity: string;
  intendedOutcome: string;
  taskId: string | null;
  projectId: string | null;
  durationMinutes: number;
  applicationPolicy: { mode: PolicyMode; entries: string[] };
  websitePolicy: { mode: PolicyMode; domains: string[] };
  networkPolicy: { mode: PolicyMode; applications: string[] };
  completionPolicy: "timer_only" | "task_confirmation";
  plannedBreaks: { durationMinutes: number; allowedApplications: string[] }[];
};
export type Version = {
  version: number;
  status: "proposed" | "approved" | "superseded";
  config: ProfileConfig & { name?: string; description?: string };
  createdAt: string;
  approvedAt: string | null;
};
export type Profile = {
  id: string;
  name: string;
  description: string;
  visible: boolean;
  sortOrder: number;
  approvedVersion: number | null;
  versions: Version[];
};
export type NamedItem = {
  id: string;
  name: string;
  description?: string | null;
  projectId?: string | null;
  completed?: boolean | null;
};
export type SessionView = {
  id: string;
  profileId: string;
  profileVersion: number;
  snapshot: ProfileConfig & { name?: string; description?: string };
  plannedDurationMinutes: number;
  startedAtMs: number;
  deadlineAtMs: number;
  state: "active_work" | "active_break" | "awaiting_completion" | "completed";
  completionPolicy: ProfileConfig["completionPolicy"];
  completionOutcome: "completed" | "partial" | "not_completed" | null;
  breakIndex: number;
  breakDeadlineAtMs: number | null;
  remainingMs: number;
  breakRemainingMs: number;
  revision: number;
};
export type SessionStatus = { active: SessionView | null; awaitingCompletion: SessionView[] };

export type SimulationState =
  | "planned"
  | "applying"
  | "applied"
  | "verified"
  | "rollback_pending"
  | "rolled_back"
  | "failed";
export type SimulationOperation = {
  operationId: string;
  requestKey: string;
  plan: {
    sessionId: string;
    profileVersion: number;
    sessionRevision: number;
    snapshotJson: string;
    rules: { family: "application" | "website" | "network"; mode: PolicyMode; entry: string }[];
  };
  state: SimulationState;
  revision: number;
  error: string | null;
};
export type SimulationRequest =
  | {
      action: "prepare";
      sessionId: string;
      expectedProfileVersion: number;
      expectedRevision: number;
      requestId: string;
    }
  | {
      action: "apply" | "verify";
      operationId: string;
      requestId: string;
      expectedOperationRevision: number;
      expectedSessionRevision: number;
    }
  | {
      action: "rollback";
      operationId: string;
      requestId: string;
      expectedOperationRevision: number;
    }
  | { action: "status"; operationId: string };

export const emptyConfig = (): ProfileConfig => ({
  activity: "Revise",
  intendedOutcome: "",
  taskId: null,
  projectId: null,
  durationMinutes: 45,
  applicationPolicy: { mode: "disabled", entries: [] },
  websitePolicy: { mode: "disabled", domains: [] },
  networkPolicy: { mode: "disabled", applications: [] },
  completionPolicy: "timer_only",
  plannedBreaks: [],
});

export function validateProfile(
  name: string,
  description: string,
  config: ProfileConfig,
): string | null {
  if (!name.trim() || name.trim().length > 80) return "Profile name must be 1–80 characters.";
  if (description.length > 1000) return "Description must be at most 1,000 characters.";
  if (!config.activity.trim() || config.activity.length > 80)
    return "Activity must be 1–80 characters.";
  if (!config.intendedOutcome.trim() || config.intendedOutcome.length > 500)
    return "Intended outcome must be 1–500 characters.";
  if (
    !Number.isInteger(config.durationMinutes) ||
    config.durationMinutes < 5 ||
    config.durationMinutes > 1440
  )
    return "Duration must be between 5 minutes and 24 hours.";
  for (const policy of [config.applicationPolicy, config.websitePolicy, config.networkPolicy]) {
    const values =
      "entries" in policy
        ? policy.entries
        : "domains" in policy
          ? policy.domains
          : policy.applications;
    if (policy.mode !== "disabled" && !values.some((v) => v.trim()))
      return "Choose at least one item for an enabled policy.";
    if (values.some((v) => v.length > 512 || v.includes("\0")))
      return "Policy entries must be at most 512 characters.";
  }
  if (
    config.plannedBreaks.length > 8 ||
    config.plannedBreaks.some(
      (b) =>
        !Number.isInteger(b.durationMinutes) || b.durationMinutes < 1 || b.durationMinutes > 180,
    )
  )
    return "Add at most 8 breaks, each 1–180 minutes.";
  return null;
}
