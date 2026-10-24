import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "./api";
import { emptyConfig, type Profile, type SimulationOperation } from "./domain";
import App from "./App";

vi.mock("./api", () => ({
  api: {
    profiles: vi.fn().mockResolvedValue([]),
    tasks: vi.fn().mockResolvedValue([]),
    projects: vi.fn().mockResolvedValue([]),
    saveProfile: vi.fn().mockResolvedValue(undefined),
    approve: vi.fn().mockResolvedValue(undefined),
    deleteProfile: vi.fn().mockResolvedValue(undefined),
    setProfileVisible: vi.fn().mockResolvedValue(undefined),
    setProfileOrder: vi.fn().mockResolvedValue(undefined),
    saveTask: vi.fn().mockResolvedValue(undefined),
    saveProject: vi.fn().mockResolvedValue(undefined),
    sessionStatus: vi.fn().mockResolvedValue({ active: null, awaitingCompletion: [] }),
    startSession: vi.fn().mockResolvedValue({}),
    shortenSession: vi.fn().mockResolvedValue({}),
    startBreak: vi.fn().mockResolvedValue({}),
    skipBreak: vi.fn().mockResolvedValue({}),
    recordOutcome: vi.fn().mockResolvedValue({}),
    simulatePolicy: vi.fn().mockResolvedValue({}),
  },
}));

describe("FOCUS by MIMI Phase 2 shell", () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    vi.mocked(api.profiles).mockResolvedValue([]);
    vi.mocked(api.tasks).mockResolvedValue([]);
    vi.mocked(api.projects).mockResolvedValue([]);
    vi.mocked(api.sessionStatus).mockResolvedValue({ active: null, awaitingCompletion: [] });
    vi.mocked(api.simulatePolicy).mockImplementation(async (request) => ({
      operationId: "simulation-1",
      requestKey: "request-1",
      plan: {
        sessionId: "session-1",
        profileVersion: 1,
        sessionRevision: 2,
        snapshotJson: "{}",
        rules: [],
      },
      state:
        request.action === "prepare"
          ? "planned"
          : request.action === "apply"
            ? "applied"
            : request.action === "verify"
              ? "verified"
              : request.action === "rollback"
                ? "rolled_back"
                : "verified",
      revision:
        request.action === "prepare"
          ? 1
          : request.action === "apply"
            ? 3
            : request.action === "verify"
              ? 4
              : 6,
      error: null,
    }));
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: query.includes("dark"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }));
  });
  it("uses the monochrome FOCUS — MIMI lockup and remains explicitly not enforcing", async () => {
    render(<App />);
    expect(screen.getByText("FOCUS — MIMI")).toBeTruthy();
    expect(screen.getAllByText("Commit. Focus. Complete.").length).toBeGreaterThan(0);
    expect(screen.getByText("NOT ENFORCING")).toBeTruthy();
    await waitFor(() => expect(api.profiles).toHaveBeenCalled());
  });
  it("persists and applies a manual theme preference", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Appearance" }), "dark");
    expect(window.localStorage.getItem("focusmimi.theme")).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("reloads the saved theme preference on remount", async () => {
    const user = userEvent.setup();
    const first = render(<App />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Appearance" }), "dark");
    first.unmount();
    render(<App />);
    expect(screen.getByRole("combobox", { name: "Appearance" })).toHaveValue("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("resolves the system theme while retaining the system preference", () => {
    render(<App />);
    expect(window.localStorage.getItem("focusmimi.theme")).toBeNull();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("creates a profile proposal with required validated fields", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "New profile" }));
    await user.type(screen.getByLabelText("Profile name"), "Revision");
    await user.type(screen.getByLabelText("Intended outcome"), "Finish chapter 1 notes");
    await user.click(screen.getByRole("button", { name: "Save proposal" }));
    await waitFor(() =>
      expect(api.saveProfile).toHaveBeenCalledWith(
        null,
        "Revision",
        "",
        expect.objectContaining({ intendedOutcome: "Finish chapter 1 notes" }),
      ),
    );
    expect(await screen.findByText(/Saved as a proposal/)).toBeTruthy();
  });
  it("shows validation feedback and does not save an incomplete profile", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "New profile" }));
    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "No outcome" } });
    fireEvent.change(screen.getByLabelText("Intended outcome"), { target: { value: "" } });
    await user.click(screen.getByRole("button", { name: "Save proposal" }));
    expect(api.saveProfile).not.toHaveBeenCalled();
    expect(await screen.findByText(/Intended outcome must be/)).toBeTruthy();
  });
  it("reviews a proposed version, compares changed fields, and approves explicitly", async () => {
    const config = { ...emptyConfig(), intendedOutcome: "Finish draft" };
    const proposal = { ...config, intendedOutcome: "Review chapter" };
    const profile: Profile = {
      id: "p1",
      name: "Revision",
      description: "Study",
      visible: true,
      sortOrder: 0,
      approvedVersion: 1,
      versions: [
        {
          version: 2,
          status: "proposed",
          config: { ...proposal, name: "Revision", description: "Study" },
          createdAt: "",
          approvedAt: null,
        },
        {
          version: 1,
          status: "approved",
          config: { ...config, name: "Revision", description: "Study" },
          createdAt: "",
          approvedAt: "",
        },
      ],
    };
    vi.mocked(api.profiles).mockResolvedValue([profile]);
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Review 1 changed field"));
    expect(screen.getByText(/intendedOutcome/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Edit proposal" }));
    const approveButton = screen.getByRole("button", { name: "Approve version 2" });
    expect(approveButton).toBeDisabled();
    await user.click(screen.getByLabelText("I reviewed the proposed changes"));
    await user.click(approveButton);
    await waitFor(() => expect(api.approve).toHaveBeenCalledWith("p1", 2));
  });
  it("creates local workspace tasks and projects through the native data API", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "Projects" }));
    await user.type(screen.getByLabelText("Projects name"), "Thesis");
    await user.click(screen.getByRole("button", { name: "Add project" }));
    await waitFor(() => expect(api.saveProject).toHaveBeenCalledWith(null, "Thesis", ""));
    await user.click(screen.getByRole("button", { name: "Tasks" }));
    await user.type(screen.getByLabelText("Tasks name"), "Outline chapter");
    await user.click(screen.getByRole("button", { name: "Add task" }));
    await waitFor(() => expect(api.saveTask).toHaveBeenCalledWith(null, "Outline chapter", null));
  });
  it("requires explicit review and confirmation before starting the exact approved version", async () => {
    const config = { ...emptyConfig(), intendedOutcome: "Finish chapter notes" };
    vi.mocked(api.profiles).mockResolvedValue([
      {
        id: "p1",
        name: "Study",
        description: "",
        visible: true,
        sortOrder: 0,
        approvedVersion: 4,
        versions: [
          {
            version: 4,
            status: "approved",
            config: { ...config, name: "Study" },
            createdAt: "",
            approvedAt: "",
          },
        ],
      },
    ]);
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Review & start session" }));
    expect(screen.getAllByText("Finish chapter notes").length).toBeGreaterThan(0);
    expect(api.startSession).not.toHaveBeenCalled();
    const start = screen.getByRole("button", { name: "Confirm and start" });
    expect(start).toBeDisabled();
    await user.click(screen.getByLabelText(/I reviewed these settings/));
    await user.click(start);
    await waitFor(() => expect(api.startSession).toHaveBeenCalledWith("p1", 4, true));
  });
  it("shows session command errors from the backend", async () => {
    vi.mocked(api.sessionStatus).mockRejectedValue(new Error("IPC unavailable"));
    render(<App />);
    expect(
      await screen.findByText(/Session status could not be loaded: Error: IPC unavailable/),
    ).toBeTruthy();
  });
  it("runs a truthful simulation and labels it as non-blocking", async () => {
    const activeSession = {
      id: "session-1",
      profileId: "p1",
      profileVersion: 1,
      snapshot: { ...emptyConfig(), intendedOutcome: "Finish notes" },
      plannedDurationMinutes: 45,
      startedAtMs: 1,
      deadlineAtMs: 2,
      state: "active_work" as const,
      completionPolicy: "timer_only" as const,
      completionOutcome: null,
      breakIndex: 0,
      breakDeadlineAtMs: null,
      remainingMs: 1000,
      breakRemainingMs: 0,
      revision: 2,
    };
    vi.mocked(api.sessionStatus).mockResolvedValue({
      active: activeSession,
      awaitingCompletion: [],
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run policy simulation" }));
    expect(await screen.findByText("verified", { exact: true })).toBeTruthy();
    expect(screen.getByText(/Apps, websites, and networks remain unrestricted/)).toBeTruthy();
    expect(screen.getByText("NOT ENFORCING")).toBeTruthy();
    expect(api.simulatePolicy).toHaveBeenCalledTimes(3);
    expect(api.simulatePolicy).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ action: "prepare" }),
    );
    expect(api.simulatePolicy).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        action: "apply",
        expectedOperationRevision: 1,
        expectedSessionRevision: 2,
      }),
    );
    expect(api.simulatePolicy).toHaveBeenNthCalledWith(
      3,
      expect.objectContaining({
        action: "verify",
        expectedOperationRevision: 3,
        expectedSessionRevision: 2,
      }),
    );
  });
  it("keeps simulation errors visible and leaves the status non-enforcing", async () => {
    const activeSession = {
      id: "session-1",
      profileId: "p1",
      profileVersion: 1,
      snapshot: { ...emptyConfig(), intendedOutcome: "Finish notes" },
      plannedDurationMinutes: 45,
      startedAtMs: 1,
      deadlineAtMs: 2,
      state: "active_work" as const,
      completionPolicy: "timer_only" as const,
      completionOutcome: null,
      breakIndex: 0,
      breakDeadlineAtMs: null,
      remainingMs: 1000,
      breakRemainingMs: 0,
      revision: 2,
    };
    vi.mocked(api.sessionStatus).mockResolvedValue({
      active: activeSession,
      awaitingCompletion: [],
    });
    vi.mocked(api.simulatePolicy).mockRejectedValueOnce(new Error("session revision is stale"));
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run policy simulation" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/session revision is stale/);
    expect(screen.getByText("NOT ENFORCING")).toBeTruthy();
  });
  it("disables duplicate simulation submission while the backend is pending", async () => {
    const activeSession = {
      id: "session-1", profileId: "p1", profileVersion: 1,
      snapshot: { ...emptyConfig(), intendedOutcome: "Finish notes" },
      plannedDurationMinutes: 45, startedAtMs: 1, deadlineAtMs: 2,
      state: "active_work" as const, completionPolicy: "timer_only" as const,
      completionOutcome: null, breakIndex: 0, breakDeadlineAtMs: null,
      remainingMs: 1000, breakRemainingMs: 0, revision: 2,
    };
    vi.mocked(api.sessionStatus).mockResolvedValue({ active: activeSession, awaitingCompletion: [] });
    let release!: (operation: SimulationOperation) => void;
    vi.mocked(api.simulatePolicy).mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<App />);
    const run = await screen.findByRole("button", { name: "Run policy simulation" });
    await user.click(run);
    const pending = screen.getByRole("button", { name: "Running simulation…" });
    expect(pending).toBeDisabled();
    await user.click(pending);
    expect(api.simulatePolicy).toHaveBeenCalledTimes(1);
    release({
      operationId: "simulation-1", requestKey: "request-1",
      plan: { sessionId: "session-1", profileVersion: 1, sessionRevision: 2, snapshotJson: "{}", rules: [] },
      state: "planned", revision: 1, error: null,
    });
    await screen.findByText("verified", { exact: true });
    expect(api.simulatePolicy).toHaveBeenCalledTimes(3);
  });
  it("keeps the confirmation review open and shows a rejected start command", async () => {
    const config = { ...emptyConfig(), intendedOutcome: "Finish notes" };
    vi.mocked(api.profiles).mockResolvedValue([
      {
        id: "p2",
        name: "Study",
        description: "",
        visible: true,
        sortOrder: 0,
        approvedVersion: 1,
        versions: [
          {
            version: 1,
            status: "approved",
            config: { ...config, name: "Study" },
            createdAt: "",
            approvedAt: "",
          },
        ],
      },
    ]);
    vi.mocked(api.startSession).mockRejectedValueOnce(new Error("profile is no longer approved"));
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Review & start session" }));
    await user.click(screen.getByLabelText(/I reviewed these settings/));
    await user.click(screen.getByRole("button", { name: "Confirm and start" }));
    expect(await screen.findByText(/profile is no longer approved/)).toBeTruthy();
    expect(screen.getByRole("dialog", { name: "Review session profile" })).toBeTruthy();
  });
});
