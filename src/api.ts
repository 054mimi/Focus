import { invoke } from "@tauri-apps/api/core";
import type {
  NamedItem,
  Profile,
  ProfileConfig,
  SessionStatus,
  SessionView,
  SimulationOperation,
  SimulationRequest,
} from "./domain";

export const api = {
  profiles: () => invoke<Profile[]>("list_profiles"),
  saveProfile: (id: string | null, name: string, description: string, config: ProfileConfig) =>
    invoke<void>("save_profile_proposal", { input: { id, name, description, config } }),
  approve: (profileId: string, version: number) =>
    invoke<void>("approve_profile_version", { profileId, version }),
  deleteProfile: (profileId: string) => invoke<void>("delete_profile", { profileId }),
  setProfileVisible: (profileId: string, visible: boolean) =>
    invoke<void>("set_profile_visible", { profileId, visible }),
  setProfileOrder: (profileIds: string[]) => invoke<void>("set_profile_order", { profileIds }),
  projects: () => invoke<NamedItem[]>("list_projects"),
  tasks: () => invoke<NamedItem[]>("list_tasks"),
  saveProject: (id: string | null, name: string, description: string) =>
    invoke<void>("save_project", { input: { id, name, description, projectId: null } }),
  saveTask: (id: string | null, name: string, projectId: string | null) =>
    invoke<void>("save_task", { input: { id, name, projectId } }),
  sessionStatus: () => invoke<SessionStatus>("get_session_status"),
  startSession: (profileId: string, profileVersion: number, confirmed: boolean) =>
    invoke<SessionView>("start_session", { profileId, profileVersion, confirmed }),
  shortenSession: (sessionId: string, expectedRevision: number, remainingMinutes: number) =>
    invoke<SessionView>("shorten_session_time", { sessionId, expectedRevision, remainingMinutes }),
  startBreak: (sessionId: string, expectedRevision: number) =>
    invoke<SessionView>("start_session_break", { sessionId, expectedRevision }),
  skipBreak: (sessionId: string, expectedRevision: number) =>
    invoke<SessionView>("skip_session_break", { sessionId, expectedRevision }),
  recordOutcome: (
    sessionId: string,
    expectedRevision: number,
    outcome: "completed" | "partial" | "not_completed",
  ) => invoke<SessionView>("record_session_outcome", { sessionId, expectedRevision, outcome }),
  simulatePolicy: (request: SimulationRequest) =>
    invoke<SimulationOperation>("simulate_session_policy", { request }),
};
