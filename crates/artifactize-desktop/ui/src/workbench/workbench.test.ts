// The workbench against its backend door: the shown worktree's terminal floats over the graph,
// collapsing or leaving it only detaches; what a terminal reports (gone with a restore offer,
// not persistent, its agent moved) shows above it, and nothing of a restore runs until the
// person presses it. The Git layer lists the worktree's history and creates a worktree from a
// commit, which opens once the Pane lists it.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import type { BranchView, LaunchView } from "../ipc/client";
import App from "../App.svelte";
import { branch, browseView } from "../monitor/fixtures";
import type { Commit, TerminalFrame } from "./backend";

const fake = vi.hoisted(() => {
  const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
  /** Each attached terminal's frame callback, by worktree. */
  const attached = new Map<string, (frame: TerminalFrame) => void>();
  const commits: Commit[] = [
    { id: "1111111aaaa", parents: ["2222222bbbb"], refs: ["HEAD", "main"], subject: "Second", author: "Ann", time: 2 },
    { id: "2222222bbbb", parents: [], refs: [], subject: "First", author: "Ann", time: 1 },
  ];
  return {
    attached,
    workbench: {
      gitHistory: vi.fn(() => ok({ commits, head: commits[0].id, more: false })),
      worktreeCreate: vi.fn(() => ok({ worktree: "/work/main-2", branch: "worktree-calm" })),
      worktreeRemove: vi.fn(() => ok(null)),
      terminalOpen: vi.fn((worktree: string, _cols: number, _rows: number, frames: (frame: TerminalFrame) => void) => {
        attached.set(worktree, frames);
        return ok(null);
      }),
      terminalInput: vi.fn(() => ok(null)),
      terminalResize: vi.fn(() => ok(null)),
      terminalClose: vi.fn((worktree: string) => {
        attached.delete(worktree);
        return ok(null);
      }),
      terminalRestore: vi.fn(() => ok(null)),
    },
  };
});
vi.mock("./backend", () => ({ workbench: fake.workbench }));

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

beforeEach(() => {
  fake.attached.clear();
  for (const command of Object.values(fake.workbench)) command.mockClear();
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
    (command, args) => {
      switch (command) {
        case "launch":
          return launch;
        case "browse":
        case "redraw":
          return browseView(++revision, { branches: listed(), launch: listed()[0].key });
        case "current_scan":
        case "current_view":
          return { branch: (args as { branch: unknown }).branch, scannedAt: 0, fingerprint: "graph:1", revision: 1, scanning: false, checking: 0, artifacts: 1, failure: null };
        case "graph":
          return {
            source: (args as { source: unknown }).source,
            shape: 1,
            update: { kind: "shape", nodes: [{ id: "a:app", name: "app", light: "reused", note: "reused", parts: [] }], edges: [] },
          };
        case "pane_complete":
          return { keep: "", candidates: [] };
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
  await waitFor(() => expect(fake.attached.has(worktree)).toBe(true));
  fake.attached.get(worktree)!(frame);
}

const terminal = () => screen.findByRole("complementary", { name: "Terminal" });
const mainTitle = () => screen.getByLabelText("Main area").querySelector("h2")?.textContent;

it("shows the shown worktree's terminal over the graph, and collapsing or leaving it only detaches", async () => {
  const [main, feature] = [branch("main", [{ kind: "current" }]), branch("feat", [{ kind: "current" }])];
  started(() => [main, feature]);
  const panel = await terminal();
  expect(within(panel).getByRole("heading").textContent).toBe("Terminal · app / main");
  await waitFor(() => expect(fake.workbench.terminalOpen).toHaveBeenCalledOnce());
  expect(fake.workbench.terminalOpen.mock.calls[0][0]).toBe("/work/main");
  // The graph is drawn beside it, not shrunk out of the way.
  await waitFor(() => expect(screen.getByLabelText("app: reused")).toBeTruthy());

  // Collapsing detaches; the tab expands it and attaches again.
  await fireEvent.click(within(panel).getByRole("button", { name: "Collapse the terminal" }));
  expect(fake.workbench.terminalClose).toHaveBeenCalledExactlyOnceWith("/work/main");
  expect(screen.queryByRole("complementary", { name: "Terminal" })).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Expand the terminal" }));
  await terminal();
  expect(fake.workbench.terminalOpen).toHaveBeenCalledTimes(2);

  // Choosing another worktree detaches this one and shows that one's terminal.
  await fireEvent.click(within(screen.getByRole("group", { name: "Pane entries" })).getByRole("button", { name: /^app \/ feat/ }));
  await waitFor(() => expect(within(screen.getByRole("complementary", { name: "Terminal" })).getByRole("heading").textContent).toBe("Terminal · app / feat"));
  expect(fake.workbench.terminalClose).toHaveBeenLastCalledWith("/work/main");
  expect(fake.workbench.terminalOpen).toHaveBeenLastCalledWith("/work/feat", expect.any(Number), expect.any(Number), expect.any(Function));
  expect(fake.workbench.terminalRestore).not.toHaveBeenCalled();
});

it("offers what a gone terminal can restore and runs only the choice the person confirms", async () => {
  started(() => [branch("main", [{ kind: "current" }])]);
  const panel = await terminal();
  await hear("/work/main", {
    kind: "restore",
    offer: { cwd: "/work/main/ui", command: "pnpm test", resume: { harness: "Claude Code", command: "claude --continue", certain: false } },
  });
  const offer = await within(panel).findByRole("region", { name: "Restore" });
  expect(offer.textContent).toContain("Rerun the last command in /work/main/ui: pnpm test");
  // Without a remembered session ID it can only offer the latest one, and says so.
  expect(offer.textContent).toContain("Resume the latest Claude Code session: claude --continue");
  expect(fake.workbench.terminalRestore).not.toHaveBeenCalled();
  await fireEvent.click(within(offer).getByRole("button", { name: "Resume the latest Claude Code session" }));
  expect(fake.workbench.terminalRestore).toHaveBeenCalledExactlyOnceWith("/work/main", { kind: "resume" });
  await waitFor(() => expect(within(panel).queryByRole("region", { name: "Restore" })).toBeNull());

  // A session known for certain resumes by its ID; keeping the fresh shell runs nothing else.
  await hear("/work/main", {
    kind: "restore",
    offer: { cwd: null, command: null, resume: { harness: "Codex", command: "codex resume 0199", certain: true } },
  });
  const again = await within(panel).findByRole("region", { name: "Restore" });
  expect(within(again).getByRole("button", { name: "Resume the Codex session" })).toBeTruthy();
  expect(within(again).queryByRole("button", { name: "Run the last command again" })).toBeNull();
  await fireEvent.click(within(again).getByRole("button", { name: "Keep the fresh shell" }));
  expect(fake.workbench.terminalRestore).toHaveBeenLastCalledWith("/work/main", { kind: "fresh" });
});

it("says when a terminal is not persistent or ended, and opens the worktree its agent moved to", async () => {
  const [main, other] = [branch("main", [{ kind: "current" }]), branch("other", [{ kind: "current" }])];
  started(() => [main, other]);
  const panel = await terminal();
  await hear("/work/main", { kind: "notPersistent", reason: "Windows kept it in the app's job" });
  expect((await within(panel).findByText("Not persistent.")).parentElement?.textContent).toContain("ends when Artifactize closes: Windows kept it in the app's job");

  await hear("/work/main", { kind: "ended", code: 0 });
  expect(await within(panel).findByText("The terminal ended with exit status 0.")).toBeTruthy();
  await fireEvent.click(within(panel).getByRole("button", { name: "Start a fresh shell" }));
  await waitFor(() => expect(fake.workbench.terminalOpen).toHaveBeenCalledTimes(2));
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
  expect(fake.workbench.gitHistory).toHaveBeenCalledWith("/work/main");

  await fireEvent.click(within(commits).getByRole("button", { name: "New worktree from 2222222" }));
  expect(fake.workbench.worktreeCreate).toHaveBeenCalledExactlyOnceWith("/work/main", "2222222bbbb");
  expect((await within(layer).findByRole("status")).textContent).toContain("Created worktree-calm in /work/main-2");

  // The Pane lists it with the next snapshot, and it opens.
  listed = [main, { ...branch("worktree-calm", [{ kind: "current" }]), workspace: "/work/main-2" }];
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(mainTitle()).toBe("Current · app  worktree-calm"));
  expect(within(layer).queryByRole("status")).toBeNull();
  await waitFor(() => expect(fake.workbench.gitHistory).toHaveBeenLastCalledWith("/work/main-2"));
});
