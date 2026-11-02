// The workbench against its backend door: the shown worktree's terminal floats over the graph,
// collapsing or leaving it only detaches; what a terminal reports (gone with a restore offer,
// not persistent, its agent moved) shows above it, and nothing of a restore runs until the
// person presses it. The Git layer lists the worktree's history and creates a worktree from a
// commit, which opens once the Pane lists it.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import type { BranchView, LaunchView } from "../ipc/client";
import App from "../App.svelte";
import { branch, browseView } from "../monitor/fixtures";
import type { Commit, TerminalFrame } from "./backend";

vi.mock("../monitor/layout", () => ({
  NODE: { width: 168, height: 44 },
  layout: async () => new Map([["a:app", { x: 0, y: 0 }]]),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {} }),
}));

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  window.matchMedia ??= ((query: string) => ({
    matches: false, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.DOMMatrixReadOnly ??= class { m22 = 1; } as unknown as typeof DOMMatrixReadOnly;
});

/** Git's history as `git_history` answers: newest first, refs as Git decorates them. */
const COMMITS: Commit[] = [
  { id: "1111111aaaa", parents: ["2222222bbbb"], refs: ["HEAD -> main", "tag: v1"], summary: "Second", time: "1793620000" },
  { id: "2222222bbbb", parents: [], refs: [], summary: "First", time: "1793610000" },
];

/** The workbench commands the app sent, with their arguments. */
let calls: { command: string; args: Record<string, unknown> }[] = [];
/** Each attached terminal's channel, by worktree. */
let attached = new Map<string, Channel<TerminalFrame>>();
const sent = (command: string) => calls.filter((call) => call.command === command).map((call) => call.args);

beforeEach(() => {
  calls = [];
  attached = new Map();
});

afterEach(() => {
  cleanup();
  clearMocks();
});

const launch: LaunchView = { mode: "graph", state: "/state", reviewer: "alice", request: null };

/** The app over a backend listing `listed()`, launched on main's Current. */
function started(listed: () => BranchView[]) {
  let revision = 0;
  mockIPC(
    (command, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      switch (command) {
        case "launch":
          return launch;
        case "browse":
        case "redraw":
          return browseView(++revision, { branches: listed(), launch: listed()[0].key });
        case "current_scan":
        case "current_view":
          return { branch: args.branch, scannedAt: 0, fingerprint: "graph:1", revision: 1, scanning: false, checking: 0, artifacts: 1, failure: null };
        case "graph":
          return {
            source: args.source,
            shape: 1,
            update: { kind: "shape", nodes: [{ id: "a:app", name: "app", light: "reused", note: "reused", parts: [] }], edges: [] },
          };
        case "pane_complete":
          return { keep: "", candidates: [] };
        case "git_history":
          calls.push({ command, args });
          return COMMITS;
        case "worktree_create":
          calls.push({ command, args });
          return { worktree: "/work/main-2", branch: "worktree-0123456789abcdef" };
        case "terminal_open":
          calls.push({ command, args });
          attached.set(args.worktree as string, args.frames as Channel<TerminalFrame>);
          return null;
        case "terminal_close":
          calls.push({ command, args });
          attached.delete(args.worktree as string);
          return null;
        case "terminal_input":
        case "terminal_resize":
        case "terminal_restore":
        case "worktree_remove":
          calls.push({ command, args });
          return null;
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  render(App);
}

/** Send `frame` to the worktree's attached terminal. */
async function hear(worktree: string, frame: TerminalFrame) {
  await waitFor(() => expect(attached.has(worktree)).toBe(true));
  attached.get(worktree)!.onmessage(frame);
}

const terminal = () => screen.findByRole("complementary", { name: "Terminal" });
const mainTitle = () => screen.getByLabelText("Main area").querySelector("h2")?.textContent;

it("shows the shown worktree's terminal over the graph, and collapsing or leaving it only detaches", async () => {
  const [main, feature] = [branch("main", [{ kind: "current" }]), branch("feat", [{ kind: "current" }])];
  started(() => [main, feature]);
  const panel = await terminal();
  expect(within(panel).getByRole("heading").textContent).toBe("Terminal · app / main");
  await waitFor(() => expect(sent("terminal_open")).toHaveLength(1));
  expect(sent("terminal_open")[0]).toMatchObject({ worktree: "/work/main", cols: expect.any(Number), rows: expect.any(Number) });
  // The host's snapshot and output reach the screen; typing goes back as bytes.
  await hear("/work/main", { kind: "snapshot", data: [...new TextEncoder().encode("\x1bc$ ")], cols: 80, rows: 24 });
  await hear("/work/main", { kind: "output", data: [...new TextEncoder().encode("ls")] });
  await waitFor(() => expect(panel.querySelector(".xterm-rows")?.textContent).toContain("$ ls"));
  await fireEvent.input(panel.querySelector("textarea")!, { data: "é", inputType: "insertText" });
  await waitFor(() => expect(sent("terminal_input")).toContainEqual({ worktree: "/work/main", data: [0xc3, 0xa9] }));
  // The graph is drawn beside it, not shrunk out of the way.
  await waitFor(() => expect(screen.getByLabelText("app: reused")).toBeTruthy());

  // Collapsing detaches; the tab expands it and attaches again. The keyboard stays where it was;
  // pressed from the keyboard, collapse hands it to the tab.
  const entries = screen.getByRole("group", { name: "Pane entries" });
  entries.focus();
  await fireEvent.click(within(panel).getByRole("button", { name: "Collapse the terminal" }));
  expect(sent("terminal_close")).toEqual([{ worktree: "/work/main" }]);
  expect(screen.queryByRole("complementary", { name: "Terminal" })).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(entries));
  await fireEvent.click(screen.getByRole("button", { name: "Expand the terminal" }));
  await terminal();
  const pressed = within(await terminal()).getByRole("button", { name: "Collapse the terminal" });
  pressed.focus();
  await fireEvent.click(pressed);
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Expand the terminal" })));
  await fireEvent.click(screen.getByRole("button", { name: "Expand the terminal" }));
  await terminal();
  await waitFor(() => expect(sent("terminal_open")).toHaveLength(3));

  // Choosing another worktree detaches this one and shows that one's terminal.
  await fireEvent.click(within(screen.getByRole("group", { name: "Pane entries" })).getByRole("button", { name: /^app \/ feat/ }));
  await waitFor(() => expect(within(screen.getByRole("complementary", { name: "Terminal" })).getByRole("heading").textContent).toBe("Terminal · app / feat"));
  expect(sent("terminal_close").at(-1)).toEqual({ worktree: "/work/main" });
  await waitFor(() => expect(sent("terminal_open").at(-1)).toMatchObject({ worktree: "/work/feat" }));
  expect(sent("terminal_restore")).toEqual([]);
});

it("offers what a gone terminal can restore and runs only the choice the person confirms", async () => {
  started(() => [branch("main", [{ kind: "current" }])]);
  const panel = await terminal();
  await hear("/work/main", {
    kind: "restoreOffer",
    offer: {
      cwd: "/work/main/ui",
      command: { program: "pnpm", args: ["test", "--filter", "a b"] },
      harness: "claudeCode",
      sessionId: null,
      resume: { program: "claude", args: ["--continue"] },
      latestSession: true,
    },
  });
  const offer = await within(panel).findByRole("region", { name: "Restore" });
  expect(offer.textContent).toContain("Rerun the last command in /work/main/ui: pnpm test --filter 'a b'");
  // Without a remembered session ID it can only offer the latest one, and says so.
  expect(offer.textContent).toContain("Resume the latest Claude Code session: claude --continue");
  expect(sent("terminal_restore")).toEqual([]);
  await fireEvent.click(within(offer).getByRole("button", { name: "Resume the latest Claude Code session" }));
  await waitFor(() => expect(sent("terminal_restore")).toEqual([{ worktree: "/work/main", choice: "resume" }]));
  await waitFor(() => expect(within(panel).queryByRole("region", { name: "Restore" })).toBeNull());

  // A session known for certain resumes by its ID; keeping the fresh shell runs nothing else.
  await hear("/work/main", {
    kind: "restoreOffer",
    offer: { cwd: null, command: null, harness: "codex", sessionId: "0199", resume: { program: "codex", args: ["resume", "0199"] }, latestSession: false },
  });
  const again = await within(panel).findByRole("region", { name: "Restore" });
  expect(within(again).getByRole("button", { name: "Resume the Codex session" })).toBeTruthy();
  expect(again.textContent).toContain("codex resume 0199");
  expect(within(again).queryByRole("button", { name: "Run the last command again" })).toBeNull();
  await fireEvent.click(within(again).getByRole("button", { name: "Keep the fresh shell" }));
  await waitFor(() => expect(sent("terminal_restore").at(-1)).toEqual({ worktree: "/work/main", choice: "freshShell" }));
});

it("says when a terminal is not persistent or ended, and opens the worktree its agent moved to", async () => {
  const [main, other] = [branch("main", [{ kind: "current" }]), branch("other", [{ kind: "current" }])];
  started(() => [main, other]);
  const panel = await terminal();
  await hear("/work/main", { kind: "notPersistent" });
  expect((await within(panel).findByText("Not persistent.")).parentElement?.textContent).toContain("ends when Artifactize closes.");

  await hear("/work/main", { kind: "ended", exitCode: 0 });
  expect(await within(panel).findByText("The terminal ended with exit status 0.")).toBeTruthy();
  await fireEvent.click(within(panel).getByRole("button", { name: "Start a fresh shell" }));
  await waitFor(() => expect(sent("terminal_open")).toHaveLength(2));
  expect(within(panel).queryByText(/The terminal ended/)).toBeNull();

  await hear("/work/main", { kind: "agentMoved", worktree: "/work/other" });
  await fireEvent.click(await within(panel).findByRole("button", { name: "Open detected worktree" }));
  await waitFor(() => expect(mainTitle()).toBe("Current · app  other"));
  expect(within(await terminal()).getByRole("heading").textContent).toBe("Terminal · app / other");
});

it("lists the worktree's history and creates a worktree from a commit, opening it once listed", async () => {
  const main = branch("main", [{ kind: "current" }]);
  let listed = [main];
  started(() => listed);
  const layer = await screen.findByRole("region", { name: "Git layer" });
  const commits = await within(layer).findByRole("list", { name: "Commits" });
  expect(within(commits).getAllByRole("listitem").map((item) => item.querySelector(".subject")?.textContent)).toEqual(["Second", "First"]);
  // Git's decorations read as chips; the checked-out commit is ringed.
  const [newest] = within(commits).getAllByRole("listitem");
  expect([...newest.querySelectorAll(".ref")].map((ref) => ref.textContent)).toEqual(["HEAD", "main", "v1"]);
  expect(newest.classList.contains("head")).toBe(true);
  expect(sent("git_history")).toContainEqual({ worktree: "/work/main" });

  await fireEvent.click(within(commits).getByRole("button", { name: "New worktree from 2222222" }));
  await waitFor(() => expect(sent("worktree_create")).toEqual([{ worktree: "/work/main", commit: "2222222bbbb" }]));
  expect((await within(layer).findByRole("status")).textContent).toContain("Created worktree-0123456789abcdef in /work/main-2");

  // The Pane lists it with the next snapshot, and it opens.
  listed = [main, { ...branch("worktree-0123456789abcdef", [{ kind: "current" }]), workspace: "/work/main-2" }];
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(mainTitle()).toBe("Current · app  worktree-0123456789abcdef"));
  expect(within(layer).getByRole("status").textContent?.replace(/\s+/g, " ").trim()).toBe("Created worktree-0123456789abcdef in /work/main-2.");
  await waitFor(() => expect(sent("git_history").at(-1)).toEqual({ worktree: "/work/main-2" }));
  expect(sent("terminal_open").at(-1)).toMatchObject({ worktree: "/work/main-2" });
});

it("offers no terminal and no worktrees for a folder outside Git", async () => {
  const folder: BranchView = { ...branch("notes", [{ kind: "current" }]), key: { kind: "folder", path: "/work/notes" }, name: "", workspace: "/work/notes" };
  started(() => [folder]);
  await waitFor(() => expect(mainTitle()).toMatch(/^Current/));
  await waitFor(() => expect(screen.getByLabelText("app: reused")).toBeTruthy());
  const layer = screen.getByRole("region", { name: "Git layer" });
  expect(layer.textContent).toContain("This folder is outside Git: it has no history and no worktrees.");
  expect(within(layer).queryByRole("button", { name: /^New worktree from/ })).toBeNull();
  expect(screen.queryByRole("complementary", { name: "Terminal" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Expand the terminal" })).toBeNull();
  expect(sent("terminal_open")).toEqual([]);
  expect(sent("git_history")).toEqual([]);
});
