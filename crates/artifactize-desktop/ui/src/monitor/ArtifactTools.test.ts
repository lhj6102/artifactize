// An Artifact's own Human tools against a mocked command boundary: what it declares in its
// worktree now, a click on a tool that runs it at once with no review, the failure it may
// answer with, and the Tool log that reaches it by its worktree and name and no other
// Artifact's, shown from its first run on.
import { tick } from "svelte";
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { ToolView } from "../ipc/client";
import { AppState } from "../shell/app.svelte";
import ArtifactTools from "./ArtifactTools.svelte";
import { branchKey } from "./fixtures";

beforeEach(() => localStorage.clear());

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

it("lists the Artifact's tools and runs one at once, its Tool log shown from the first run on and unfolded", async () => {
  const runs: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "artifact_tools") return [notes];
    if (command === "artifact_tool_run") {
      runs.push(args);
      return { text: "Started open.", error: false };
    }
    return null;
  });
  const { app } = renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "notes" });
  // A person folded the Tool log in an earlier Detail.
  app.sections.set("Tool log", false);
  const tools = await screen.findByRole("region", { name: "Tools of notes" });
  // Before any run there is no Tool log.
  expect(screen.queryByRole("button", { name: "Tool log" })).toBeNull();
  await fireEvent.click(await within(tools).findByRole("button", { name: "open" }));
  await waitFor(() => expect(runs).toEqual([{ branch: branchKey("main"), artifact: "notes", tool: "open_notes", job: expect.any(Number) }]));
  // What ended well is the log's to say: no notice beside the tools.
  expect(within(tools).queryByText("Started open.")).toBeNull();
  app.logged([{ key: { kind: "artifact", workspace: "/work/main", artifact: "notes" }, entries: [{ run: "1", at: 0, operation: "open", artifact: "notes", lines: [], end: { text: "■ launched", error: false } }] }]);
  await screen.findByRole("log", { name: "Tool log" });
  expect(screen.getByRole("button", { name: "Tool log" }).getAttribute("aria-expanded")).toBe("true");
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
  await fireEvent.click(await within(tools).findByRole("button", { name: "open" }));
  await waitFor(() => expect(runs).toEqual([{ branch: branchKey("feat/y"), artifact: "notes", tool: "open_notes", job: expect.any(Number) }]));
});

it("shows no Tools section when the Artifact declares no Human tools", async () => {
  mockIPC((command) => (command === "artifact_tools" ? [] : null));
  const { app } = renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "core" });
  // The owner has read the Artifact's tools, none, and the component has drawn what it read.
  await waitFor(() => expect(app.knownTools("/work/main", "core")?.tools).toEqual([]));
  await tick();
  expect(screen.queryByRole("region", { name: "Tools of core" })).toBeNull();
  expect(screen.queryByText("Tools")).toBeNull();
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

it("shares one read between the Details that open the Artifact while it runs", async () => {
  let reads = 0;
  let answer: (tools: ToolView[]) => void = () => {};
  mockIPC((command) => {
    if (command !== "artifact_tools") return null;
    reads += 1;
    return new Promise<ToolView[]>((resolve) => (answer = resolve));
  });
  const tools = new AppState().artifactTools("/work/main", "notes");
  const first = tools.load(branchKey("main"));
  const second = tools.load(branchKey("main"));
  await waitFor(() => expect(reads).toBe(1));
  answer([notes]);
  await Promise.all([first, second]);
  expect(tools.tools).toEqual([notes]);
  expect(reads).toBe(1);
});

it("reads the tools again after a run only when the run failed", async () => {
  let reads = 0;
  let ran = 0;
  let notice = { text: "Started open.", error: false };
  mockIPC((command) => {
    if (command === "artifact_tools") {
      reads += 1;
      return [notes];
    }
    if (command === "artifact_tool_run") return (ran += 1, notice);
    return null;
  });
  renderHosted(ArtifactTools, { branch: branchKey("main"), workspace: "/work/main", artifact: "notes" });
  const tools = await screen.findByRole("region", { name: "Tools of notes" });
  const run = await within(tools).findByRole("button", { name: "open" });
  await fireEvent.click(run);
  await waitFor(() => expect(ran).toBe(1));
  await waitFor(() => expect((run as HTMLButtonElement).disabled).toBe(false));
  // What ended well is not said.
  expect(within(tools).queryByRole("status")).toBeNull();
  expect(reads).toBe(1);
  notice = { text: "open_notes did not run.", error: true };
  await fireEvent.click(run);
  await waitFor(() => expect(reads).toBe(2));
  expect(within(tools).getByRole("status").textContent).toBe("open_notes did not run.");
});
