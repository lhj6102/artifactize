// An Artifact's own Human tools against a mocked command boundary: what it declares in its
// worktree now, a ▶ click that runs one at once with no review, the notice it answers with,
// and the Tool log that reaches it by its worktree and name and no other Artifact's.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import type { ToolView } from "../ipc/client";
import { AppState } from "../shell/app.svelte";
import ArtifactTools from "./ArtifactTools.svelte";
import { branchKey } from "./fixtures";

afterEach(() => {
  cleanup();
  clearMocks();
});

const notes: ToolView = {
  name: "open_notes",
  artifact: "notes",
  operation: "open",
  kind: "launch",
  description: "Open the release notes",
  action: "xdg-open notes.md",
  error: null,
};

it("lists the Artifact's tools and runs one at once, showing its notice", async () => {
  const runs: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "artifact_tools") return [notes];
    if (command === "artifact_tool_run") {
      runs.push(args);
      return { text: "Started open.", error: false };
    }
    return null;
  });
  renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "notes" });
  const tools = await screen.findByRole("region", { name: "Tools of notes" });
  await fireEvent.click(await within(tools).findByRole("button", { name: "▶ open" }));
  await waitFor(() => expect(within(tools).getByRole("status").textContent).toBe("Started open."));
  expect(runs).toEqual([{ branch: branchKey("main"), artifact: "notes", tool: "open_notes", job: expect.any(Number) }]);
  expect(screen.getByRole("log", { name: "Tool log" })).toBeTruthy();
});

it("runs a tool on the branch checked out in the worktree now", async () => {
  const runs: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "artifact_tools") return [notes];
    if (command === "artifact_tool_run") {
      runs.push(args);
      return { text: "Started open.", error: false };
    }
    return null;
  });
  const { rerender } = renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "notes" });
  const tools = await screen.findByRole("region", { name: "Tools of notes" });
  // Another branch is checked out in the same worktree: the same tools, a new target.
  await rerender({ branch: branchKey("feat/y") });
  await fireEvent.click(await within(tools).findByRole("button", { name: "▶ open" }));
  await waitFor(() => expect(runs).toEqual([{ branch: branchKey("feat/y"), artifact: "notes", tool: "open_notes", job: expect.any(Number) }]));
});

it("says so when the Artifact declares no Human tools", async () => {
  mockIPC((command) => (command === "artifact_tools" ? [] : null));
  renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "core" });
  await screen.findByText("core declares no Human tools.");
});

it("routes a Tool log to the Artifact of its worktree only", () => {
  const app = new AppState();
  const main = app.artifactTools("/work/main", "notes");
  const other = app.artifactTools("/work/feat-x", "notes");
  const entry = { run: "1", at: 0, operation: "open", artifact: "notes", lines: [{ text: "opened", error: false }], end: null };
  app.logged([{ key: { kind: "artifact", workspace: "/work/main", artifact: "notes" }, entries: [entry] }]);
  expect(main.log).toEqual([entry]);
  expect(other.log).toEqual([]);
  // The same worktree and Artifact are one owner, whichever Detail asks again.
  expect(app.artifactTools("/work/main", "notes")).toBe(main);
});
