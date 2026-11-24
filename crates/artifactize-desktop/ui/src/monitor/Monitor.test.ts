// The window against a mocked command boundary: Verify's Pane lists the branches the backend
// sends, expands them to their timelines and moves one selection through them; a timeline entry
// shows its Run; Open in Chat finds the branch's Current, which can be refreshed or rescan when a
// Run of its branch ends; the search's Esc puts sort:recent back. The bottom bar switches
// modes, each kept as it was left, and counts the waiting reviews, which Review lists and opens.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { BrowseView, ChatSession, ChatSessionView, CurrentView, LaunchView, Navigate, RunPanel } from "../ipc/client";
import App from "../App.svelte";
import { branch, branchKey, browseView, runChip, waitingRow } from "./fixtures";
import { draftLibrary, humanView } from "../review/fixtures";

vi.mock("./layout", () => ({
  NODE: { width: 168, height: 44 },
  layout: async () => new Map([["a:app", { x: 0, y: 0 }], ["a:graph", { x: 240, y: 0 }]]),
}));

/** Every title the app gave its window, oldest first. */
const windowTitles = vi.hoisted(() => [] as string[]);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {}, setTitle: async (title: string) => void windowTitles.push(title) }),
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

afterEach(() => {
  cleanup();
  clearMocks();
});

/** The fixed instant of the scan with this revision. */
const scannedAt = (revision: number) => Date.UTC(2026, 9, 21, 9, 0, revision);

const current = (revision: number, checking = 0): CurrentView => ({
  branch: branchKey("main"),
  scannedAt: scannedAt(revision),
  fingerprint: `graph:revision-${revision}`,
  revision,
  scanning: checking > 0,
  checking,
  artifacts: 2,
  failure: null,
});

/** The time a Current's header shows for a scan, as the app writes it. */
const clock = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });

const saved = {
  id: "chat-main", title: "Work on main", worktree: "/work/main", project: "app", branch: "main",
  branchKey: branchKey("main"), view: "conversation", unavailable: null,
  model: { backend: "fixture", model: "small", effort: null },
  createdAt: "2026-11-21T00:00:00Z", updatedAt: "2026-11-21T00:00:00Z", running: false, outcome: null, unseen: false,
} satisfies ChatSession;

/** The backend: `snapshot` answers every browse; the calls are recorded. */
function backend(launch: LaunchView, snapshot: () => BrowseView, steps: { view: (scan: number) => CurrentView } = { view: current }, outside: string | null = null, latest: ChatSession | null = saved, open?: (id: string) => Promise<BrowseView> | undefined) {
  const calls: { command: string; args: unknown }[] = [];
  let scans = 0;
  let chat = latest;
  /** The newest step the backend holds. */
  let shown: CurrentView | null = null;
  let stage: "claim" | "review" = "claim";
  const library = draftLibrary();
  mockIPC(
    (command, args) => {
      calls.push({ command, args });
      const drafted = library.answer(command, args);
      if (drafted) return drafted;
      switch (command) {
        case "launch":
          return launch;
        case "human_search":
          return {
            reviews: snapshot().waiting.map((row) => ({
              id: row.id, target: { run: "run-0", node: "e:release/signoff" }, title: row.title, eval: row.eval,
              worktree: "/app", project: row.project, branch: row.branch, age: row.age, mark: row.mark, claimant: null, mine: false,
            })),
            next: null,
          };
        case "chat_list":
          return chat ? [chat] : [];
        case "chat_latest_for_branch":
          return chat;
        case "chat_set_view":
          chat = chat && { ...chat, view: (args as { view: ChatSessionView }).view };
          return chat;
        case "chat_open":
          return { session: chat, running: false, turns: [] };
        case "chat_branches":
          return [branch("other", []), branch("main", [])];
        case "chat_default_model":
          return saved.model;
        case "human_waiting_count":
          return snapshot().waiting.length;
        case "human_open_reference":
          return { view: humanView(stage), outsideFilter: outside !== null, outsideReason: outside };
        case "detail": {
          const asked = args as { source: unknown; node: string };
          return {
            source: asked.source, node: asked.node, target: "artifact", title: asked.node, summary: "", request: null, profile: null,
            mark: null, plan: null, outcome: null, sections: [], evidence: null, card: null, running: false,
          };
        }
        case "request_detail":
          return { profile: "human", request: (args as { id: string }).id };
        case "browse": {
          // A Run opened by its ID is read whatever the Pane lists, with its live branch.
          const step = (args as { navigate?: Navigate }).navigate?.run;
          const view = snapshot();
          if (step?.kind === "close") return { ...view, run: { kind: "none" } };
          if (step?.kind !== "open" && step?.kind !== "reference") return view;
          const pending = open?.(step.id);
          if (pending) return pending;
          if (view.run.kind === "read" && view.run.id === step.id) return view;
          const live = view.branches.find((each) => each.chips.some((chip) => chip.kind === "run" && chip.id === step.id));
          return { ...view, run: { kind: "read", id: step.id, origin: null, headline: null, node: `r:${step.id}`, mark: { status: "GREEN", glyph: "✓" }, needs: [], branch: live?.key ?? null, chatBranch: live?.key ?? branchKey("main") } };
        }
        case "redraw":
          return snapshot();
        case "current_scan":
          shown = steps.view(++scans);
          return shown;
        case "current_view":
          return shown;
        case "graph":
          return {
            source: (args as { source: unknown }).source,
            shape: 1,
            update: {
              kind: "shape",
              nodes: [
                { id: "a:app", name: "app", light: "reused", tally: { passed: 1, of: 1 }, note: "reused", parts: [] },
                { id: "a:graph", name: "graph", light: "run", tally: { passed: 0, of: 1 }, note: "would run (1 eval)", parts: [] },
              ],
              edges: [{ source: "a:app", target: "a:graph", light: "reused" }],
            },
          };
        case "pane_complete":
          return { keep: "", candidates: [] };
        case "human_open":
          return humanView(stage);
        case "human_claim":
          stage = "review";
          return { notice: null, view: humanView(stage) };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  const navigations = () => calls.filter((call) => call.command === "browse").map((call) => (call.args as { navigate: Navigate }).navigate);
  const scanned = () => calls.filter((call) => call.command === "current_scan").length;
  /** The running scan publishes `view`. */
  const publish = async (view: CurrentView) => {
    shown = view;
    await emit("current-changed", { branch: view.branch });
  };
  return { calls, navigations, scanned, publish };
}

/** A mode's workspace, shown or hidden. */
/** The Run the Pane's shown entry names, filled in its timeline: the stage header names nothing (J-STAGE, J-MAIN-RUN). */
function shownRun(): string | null {
  const entry = workspace("Verify").querySelector('[aria-label="Pane entries"] button[data-chip][aria-pressed="true"]');
  return entry?.getAttribute("data-chip") ?? null;
}

/** The review the Review Pane's selected row names. */
function shownReview(): string | null {
  return workspace("Review").querySelector('[role="listbox"][aria-label="Human reviews"] [aria-selected="true"]')?.textContent ?? null;
}

function workspace(mode: string): HTMLElement {
  const found = document.querySelector<HTMLElement>(`main[aria-label="${mode}"]`);
  if (!found) throw new Error(`no ${mode} workspace`);
  return found;
}

const monitor = (request: string | null = null): LaunchView => ({ mode: "monitor", state: "/state", reviewer: "alice", request, graphBranch: null });

it("opens on the Current of the branch it was started for, and scans it again on Refresh, Ctrl+R and F5", async () => {
  const main = branch("main", []);
  const { calls, scanned, navigations } = backend({ ...monitor(), mode: "graph" }, () => browseView(1, { branches: [main], launch: main.key }));
  render(App);
  /** The header row shows the time of the scan with this revision, read aloud as when it was
   * scanned, without a sentence. */
  const header = async (revision: number) => {
    const time = await screen.findByLabelText(/^Scanned at /);
    await waitFor(() => expect(time.textContent?.trim()).toBe(clock(scannedAt(revision))));
    expect(time.getAttribute("aria-label")).toBe(`Scanned at ${clock(scannedAt(revision))}`);
    expect(time.getAttribute("datetime")).toBe(new Date(scannedAt(revision)).toISOString());
  };
  await header(1);
  expect(workspace("Chat").hidden).toBe(false);
  // The Pane's selected row names the session; the stage header does not (J-STAGE).
  expect(screen.getByLabelText("Chats").querySelector('[aria-current="true"] .title')?.textContent).toBe("Work on main");
  expect(screen.getByLabelText("Chat's main area").querySelector("header")?.textContent).not.toContain("Work on main");
  await waitFor(() => expect(screen.getByLabelText("graph: would run (1 eval)")).toBeTruthy());
  expect(screen.getByLabelText("app: reused")).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Graph" }).getAttribute("aria-selected")).toBe("true");
  // No structure text: the graph is the Current's whole.
  expect(screen.queryByText(/Structure and evaluation criteria/)).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: /^Refresh/ }));
  await waitFor(() => expect(scanned()).toBe(2));
  await header(2);
  await fireEvent.keyDown(window, { key: "r", ctrlKey: true });
  await waitFor(() => expect(scanned()).toBe(3));
  await header(3);
  await fireEvent.keyDown(window, { key: "F5" });
  await waitFor(() => expect(scanned()).toBe(4));
  await header(4);
  // Only a person's refresh asks for the files as they are now, even past a scan still running.
  expect(calls.filter((call) => call.command === "current_scan").map((call) => (call.args as { refresh: boolean }).refresh))
    .toEqual([false, true, true, true]);
  // Each manual refresh also read the state again with every cache dropped.
  expect(navigations().filter((step) => step.resync)).toHaveLength(2);
});

it("draws a Current's graph at once, counting the Artifacts still being checked until its scan ends", async () => {
  const main = branch("main", []);
  const { publish } = backend({ ...monitor(), mode: "graph" }, () => browseView(1, { branches: [main], launch: main.key }), {
    view: (scan) => current(scan, 2),
  });
  render(App);
  // A count that falls, read aloud as how many Artifacts are left: no sentence, no ratio.
  await waitFor(() => expect(screen.getByLabelText("Checking, 2 Artifacts left").textContent?.trim()).toBe("2 left"));
  await waitFor(() => expect(screen.getByLabelText("graph: would run (1 eval)")).toBeTruthy());
  expect(screen.getByRole("button", { name: /^Refresh/ }).hasAttribute("disabled")).toBe(false);
  await publish(current(2, 1));
  await waitFor(() => expect(screen.getByLabelText("Checking, 1 Artifacts left").textContent?.trim()).toBe("1 left"));
  await publish({ ...current(3), failure: "Fingerprint script for Artifact app failed." });
  await waitFor(() => expect(screen.getByText("Fingerprint script for Artifact app failed.")).toBeTruthy());
  // The graph stays.
  expect(screen.getByLabelText("app: reused")).toBeTruthy();
  expect(screen.getByRole("button", { name: /^Refresh/ }).hasAttribute("disabled")).toBe(false);
});

it("expands a clicked branch to its timeline and moves through the timelines with the keys", async () => {
  const feature = branch("feat/x", [runChip("run-2", "RUNNING", true, "Fixing the login form"), runChip("run-1", "GREEN")]);
  const main = branch("main", [runChip("run-0", "RED")]);
  const { navigations, scanned } = backend(monitor(), () => browseView(1, { branches: [feature, main], waiting: [waitingRow("run-0-1")] }));
  render(App);
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  const name = (text: RegExp) => within(entries).getByRole("button", { name: text });
  const header = shownRun;
  // Collapsed, a branch shows its newest Run's status under its name.
  await waitFor(() => expect(name(/^app \/ feat\/x/).textContent).toMatch(/Running\s*5m/));
  expect(name(/^app \/ main/).textContent).toMatch(/RED\s*5m ago/);
  expect(screen.queryByRole("group", { name: /^Runs of/ })).toBeNull();
  expect(header()).toBeNull();
  expect(within(screen.getByLabelText("Main area")).getByText("Choose a branch in the Pane.")).toBeTruthy();
  // A click on a branch expands it and shows its newest entry: the Run in progress.
  await fireEvent.click(name(/^app \/ feat\/x/));
  await waitFor(() => expect(header()).toBe("run-2"));
  expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-2" } });
  const timeline = screen.getByRole("group", { name: "Runs of app feat/x" });
  // Each entry: its status and age, then its comment, or else its ID.
  expect(within(timeline).getAllByRole("button").map((entry) => entry.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "Running 5m Fixing the login form",
    "GREEN 5m ago run-1",
  ]);
  // ↓ enters the timeline, then steps to the older Run; ← goes back to the branch, ← again folds it.
  entries.focus();
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(timeline.querySelector(':scope > [role="treeitem"][aria-selected="true"] button[data-chip="run-2"]')).not.toBeNull());
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(header()).toBe("run-1"));
  expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-1" } });
  await fireEvent.keyDown(entries, { key: "ArrowLeft" });
  await fireEvent.keyDown(entries, { key: "ArrowLeft" });
  await waitFor(() => expect(screen.queryByRole("group", { name: "Runs of app feat/x" })).toBeNull());
  expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-1" } });
  // ↓ to the next branch shows its newest Run; → expands it and → enters the timeline.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-0" } }));
  expect(scanned()).toBe(0);
  await fireEvent.keyDown(entries, { key: "ArrowRight" });
  expect(screen.getByRole("group", { name: "Runs of app main" })).toBeTruthy();
  await fireEvent.keyDown(entries, { key: "ArrowRight" });
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-0" } }));
  // Verify's Pane has no waiting reviews: ↓ past the last entry stays on it.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  expect(header()).toBe("run-0");
  expect(screen.queryByRole("listbox", { name: "Waiting reviews" })).toBeNull();
});

it("reopens Settings on Providers from the bar and shortcut", async () => {
  backend(monitor(), () => browseView(1));
  render(App);
  const gear = await screen.findByRole("button", { name: "Settings" });
  await fireEvent.click(gear);
  const settings = await screen.findByRole("dialog", { name: "Settings" });
  await fireEvent.click(within(settings).getByRole("tab", { name: "Remote" }));
  expect(within(settings).getByRole("tab", { name: "Remote" }).getAttribute("aria-selected")).toBe("true");
  await fireEvent.click(within(settings).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Settings" })).toBeNull());
  await fireEvent.keyDown(window, { key: ",", ctrlKey: true });
  const reopened = await screen.findByRole("dialog", { name: "Settings" });
  expect(within(reopened).getByRole("tab", { name: "Providers" }).getAttribute("aria-selected")).toBe("true");
});

it("lists the composer's shortcuts in Conversation Help instead of branch navigation", async () => {
  backend(monitor(), () => browseView(1));
  render(App);
  await fireEvent.click(within(await screen.findByRole("navigation", { name: "Modes" })).getByRole("button", { name: "Chat" }));
  await fireEvent.keyDown(window, { key: "?" });
  const help = await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
  const conversation = within(help).getByRole("heading", { name: "Conversation" }).parentElement!;
  expect([...conversation.querySelectorAll("dd")].map((action) => action.textContent)).toEqual(["Send", "Break a line", "Stop the running turn", "Find in the conversation"]);
  expect(within(help).queryByRole("heading", { name: "Pane" })).toBeNull();
});

it("switches modes from the bottom bar, each kept as it was left, and lists and opens the waiting reviews in Review", async () => {
  const main = branch("main", [runChip("run-0", "RED")]);
  backend(monitor(), () => browseView(1, { branches: [main], waiting: [waitingRow("run-0-1")] }));
  render(App);
  const modes = await screen.findByRole("navigation", { name: "Modes" });
  const mode = (name: RegExp) => within(modes).getByRole("button", { name });
  const settings = screen.getByRole("button", { name: "Settings" });
  expect(settings.title).toBe("Settings");
  expect(settings.textContent?.trim()).toBe("");
  expect(settings.querySelector("svg")?.getAttribute("data-icon")).toBe("settings");
  // Verify shows first; Review carries the count of the reviews waiting.
  expect(mode(/^Verify/).getAttribute("aria-current")).toBe("page");
  await waitFor(() => expect(mode(/^Review/).getAttribute("aria-label")).toBe("Review, 1 waiting"));
  await fireEvent.click(await within(screen.getByRole("group", { name: "Pane entries" })).findByRole("button", { name: /^app \/ main/ }));
  await waitFor(() => expect(shownRun()).toBe("run-0"));
  // Review lists the waiting review; a click opens it in Review's main area.
  await fireEvent.click(mode(/^Review/));
  expect(mode(/^Review/).getAttribute("aria-current")).toBe("page");
  expect(workspace("Verify").hidden).toBe(true);
  const list = screen.getByRole("listbox", { name: "Human reviews" });
  await fireEvent.click(within(list).getByRole("option"));
  const review = screen.getByLabelText("Review's main area");
  await within(review).findByLabelText("Human review");
  // Verify again: its Run is shown as it was left; Ctrl+3 shows the review again.
  await fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
  expect(workspace("Verify").hidden).toBe(false);
  expect(shownRun()).toBe("run-0");
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  expect(workspace("Review").hidden).toBe(false);
  expect(within(review).getByLabelText("Human review")).toBeTruthy();
});

it.each([[1, "1"], [99, "99"], [100, "99+"]] as const)("shows %i waiting reviews as %s without shortening the accessible name", async (count, badge) => {
  backend(monitor(), () => browseView(1, { waiting: Array.from({ length: count }, (_, i) => waitingRow(`run-0-${i}`)) }));
  render(App);
  const review = await screen.findByRole("button", { name: `Review, ${count} waiting` });
  expect(within(review).getByText(badge).getAttribute("aria-hidden")).toBe("true");
});

it.each(["missing", "rejected"] as const)("shows the chosen Run loading before its %s read, not the previous graph", async (result) => {
  const first = branch("main", [runChip("run-old", "GREEN")]);
  const next = branch("feature", [runChip("run-new", "RED")]);
  let settle!: () => void;
  const held = new Promise<void>((resolve) => { settle = resolve; });
  backend(monitor(), () => browseView(1, { branches: [first, next] }), undefined, null, saved, (id) => id === "run-new" ? held.then(() => {
    if (result === "rejected") throw { kind: "read", message: "Recorded Run is unreadable." };
    return browseView(2, { branches: [first, next], run: { kind: "missing" } });
  }) : undefined);
    render(App);
    await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
    await screen.findByLabelText("app: reused");
    await fireEvent.click(screen.getByRole("button", { name: /^app \/ feature/ }));
    const main = within(screen.getByLabelText("Main area"));
    expect(shownRun()).toBe("run-new");
    expect(main.getByText("Loading the Run…")).toBeTruthy();
    expect((main.getByRole("button", { name: /^Zoom in/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByLabelText("app: reused")).toBeNull();
    settle();
    await main.findByText("This Run cannot be read; choose another.");
    expect(shownRun()).toBe("run-new");
});

it("names the Run by its Pane entry only, with the graph's tools, Run details and Open in Chat in the stage header", async () => {
  const feature = branch("feat/x", [runChip("run-2", "YELLOW", false, "Fixing the login form")]);
  const run: RunPanel = {
    id: "run-2",
    branch: feature.key,
    chatBranch: feature.key,
    origin: null,
    headline: null,
    node: "run:run-2",
    mark: { status: "YELLOW", glyph: "✓" },
    needs: [
      { mark: { status: "RED", glyph: "✗" }, text: "cli/tests" },
      { mark: { status: "DEFERRED", glyph: "?" }, text: "release/signoff" },
    ],
  };
  backend(monitor(), () => browseView(1, { branches: [feature], run: { kind: "read", ...run } }));
  render(App);
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  await fireEvent.click(await within(entries).findByRole("button", { name: /^app \/ feat\/x/ }));
  await waitFor(() => expect(shownRun()).toBe("run-2"));
  const main = screen.getByLabelText("Main area");
  const tools = within(main).getByRole("toolbar", { name: "Tools" });
  // Controls only: the header names nothing and rates nothing (J-STAGE).
  expect(main.querySelector("header h2, header [role=heading]")).toBeNull();
  expect(main.querySelector("header")?.textContent).not.toMatch(/run-2|YELLOW|cli\/tests/);
  // The graph's tools, then the Run's own: Run details, and Open in Chat last.
  const names = within(tools).getAllByRole("button").map((tool) => tool.getAttribute("aria-label") ?? tool.textContent?.trim());
  expect(names.slice(-2)).toEqual(["Run details", "Open in Chat"]);
  expect(names).toEqual(expect.arrayContaining(["Zoom out", "Zoom in"]));
  // Run details opens the Run's Detail, shows it on while open, and closes it again.
  const details = within(tools).getByRole("button", { name: "Run details" }) as HTMLButtonElement;
  await waitFor(() => expect(details.disabled).toBe(false));
  expect(details.getAttribute("aria-pressed")).toBe("false");
  await fireEvent.click(details);
  await waitFor(() => expect(details.getAttribute("aria-pressed")).toBe("true"));
  expect(screen.getByRole("complementary", { name: "Detail" })).toBeTruthy();
  await fireEvent.click(details);
  await waitFor(() => expect(details.getAttribute("aria-pressed")).toBe("false"));
  expect(screen.queryByRole("complementary", { name: "Detail" })).toBeNull();
});

it("stars a branch from its entry, and takes a star away", async () => {
  const starred = branch("feat/x", [], false, true);
  const main = branch("main", []);
  const { navigations } = backend(monitor(), () => browseView(1, { branches: [starred, main] }));
  render(App);
  const star = await screen.findByRole("button", { name: "Star app / main" });
  expect(star.getAttribute("aria-pressed")).toBe("false");
  await fireEvent.click(star);
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ star: { branch: main.key, on: true } }));
  const on = screen.getByRole("button", { name: "Star app / feat/x" });
  expect(on.getAttribute("aria-pressed")).toBe("true");
  await fireEvent.click(on);
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ star: { branch: starred.key, on: false } }));
});

it("asks for older Runs from Older runs, and from ↓ past a timeline's oldest entry", async () => {
  const main = branch("main", [runChip("run-1", "GREEN")], true);
  const later = branch("later", []);
  const { navigations } = backend(monitor(), () => browseView(1, { branches: [main, later] }));
  render(App);
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
  await waitFor(() => expect(shownRun()).toBe("run-1"));
  await fireEvent.click(screen.getByRole("button", { name: "Older runs" }));
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ older: main.key }));
  // At the oldest entry, ↓ asks again instead of leaving for the next branch.
  const asked = navigations().filter((step) => step.older).length;
  entries.focus();
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(screen.getByRole("group", { name: "Runs of app main" }).querySelector(':scope > [role="treeitem"][aria-selected="true"] button[data-chip="run-1"]')).not.toBeNull());
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().filter((step) => step.older)).toHaveLength(asked + 1));
  expect(shownRun()).toBe("run-1");
});

it("opens the latest session in Graph from a Run header, and rescans when that branch's Run ends", async () => {
  let listed = true;
  let revision = 0;
  const main = branch("main", [runChip("run-1", "RUNNING", true)]);
  const { calls, scanned, navigations } = backend(monitor(), () => browseView(++revision, { branches: listed ? [main] : [] }));
  render(App);
  await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
  const header = screen.getByLabelText("Main area").querySelector("header")!;
  await fireEvent.click(await within(header).findByRole("button", { name: "Open in Chat" }));
  await waitFor(() => expect(scanned()).toBe(1));
  expect(calls.filter(({ command }) => command === "chat_latest_for_branch" || command === "chat_set_view"))
    .toEqual([{ command: "chat_latest_for_branch", args: { branch: main.key } }, { command: "chat_set_view", args: { id: saved.id, view: "graph" } }]);
  expect(workspace("Chat").hidden).toBe(false);
  expect(screen.getByRole("tab", { name: "Graph" }).getAttribute("aria-selected")).toBe("true");
  listed = false;
  const reads = navigations().length;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(navigations().length).toBeGreaterThan(reads));
  await emit("run-finished", { run: "run-other", branch: branch("other", []).key });
  expect(scanned()).toBe(1);
  await emit("run-finished", { run: "run-1", branch: main.key });
  await waitFor(() => expect(scanned()).toBe(2));
});

it.each(["reference", "graph launch"])("opens New chat on the target branch when %s finds no session", async (entry) => {
  const other = branch("other", [runChip("run-other", "GREEN")]);
  const main = branch("main", []);
  const launched = entry === "graph launch";
  const { calls } = backend({ ...monitor(), mode: launched ? "graph" : "monitor" }, () => browseView(1, { branches: launched ? [other] : [main], launch: launched ? main.key : null }), { view: current }, null, null);
  render(App);
  if (!launched) {
    await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
    expect(await screen.findByText("No Runs on this branch yet.")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Open in Chat" }));
  }
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  expect(workspace("Chat").hidden).toBe(false);
  const choices = within(dialog).getByRole<HTMLSelectElement>("combobox", { name: "Branch" });
  await waitFor(() => expect(choices.value).toBe(main.workspace));
  expect([...choices.options].map((option) => option.value)).toEqual([other.workspace, main.workspace]);
  expect(calls.find(({ command }) => command === "chat_latest_for_branch")?.args).toEqual({ branch: main.key });
  expect(calls.find(({ command }) => command === "chat_default_model")?.args).toEqual({ worktree: main.workspace, branch: main.key });
  expect(calls.some(({ command }) => command === "chat_set_view" || command === "current_scan")).toBe(false);
});

it("searches the Pane as typed, and Esc puts sort:recent back before leaving the field", async () => {
  const { navigations } = backend(monitor(), () => browseView(1, { branches: [branch("main", [])] }));
  render(App);
  const field = await screen.findByLabelText<HTMLInputElement>("Search the branches");
  expect(field.value).toBe("sort:recent");
  await fireEvent.keyDown(window, { key: "/" });
  expect(document.activeElement).toBe(field);
  await fireEvent.input(field, { target: { value: "branch:main" } });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ query: "branch:main" }));
  await fireEvent.keyDown(field, { key: "Escape" });
  expect(field.value).toBe("sort:recent");
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ query: "sort:recent" }));
  await fireEvent.keyDown(field, { key: "Escape" });
  expect(document.activeElement).toBe(screen.getByRole("group", { name: "Pane entries" }));
});

it("opens the newest waiting review when started with review alone", async () => {
  backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1"), waitingRow("run-8-1")] }));
  render(App);
  const main = await screen.findByLabelText("Review's main area");
  // The Pane's selected row names the review's eval; the column's head says its title.
  await waitFor(() => expect(shownReview()).toMatch(/release\/signoff/));
  expect(main.querySelector("header")?.textContent).not.toMatch(/release\/signoff/);
  expect(workspace("Review").hidden).toBe(false);
  const rows = within(screen.getByRole("listbox", { name: "Human reviews" })).getAllByRole("option");
  expect(rows[0].getAttribute("aria-selected")).toBe("true");
  expect(within(main).getByLabelText("Human review")).toBeTruthy();
});

it("opens a review by reference without changing the filter or adding a banner, and Open in Verify shows its Run", async () => {
  const main = branch("main", [runChip("run-a", "GREEN")]);
  backend({ ...monitor("run-a-2"), mode: "review" }, () => browseView(1, { branches: [main] }), { view: current }, "claimed by you");
  render(App);
  const area = await screen.findByLabelText("Review's main area");
  await within(area).findByLabelText("Human review");
  expect(within(area).queryByText(/outside the filter/)).toBeNull();
  // The filter is left as it was.
  expect((screen.getByLabelText("Filter the reviews") as HTMLInputElement).value).toBe("type:human waiting");
  // Not followed from Verify: no way back, but Open in Verify.
  expect(within(area).queryByRole("button", { name: /^Verify · / })).toBeNull();
  await fireEvent.click(within(area).getByRole("button", { name: "Open in Verify" }));
  expect(workspace("Verify").hidden).toBe(false);
  await waitFor(() => expect(shownRun()).toBe("run-a"));
});

it("holds Find and Open in Verify in a review's stage header; Esc leaves the reasons field, then returns to the Pane", async () => {
  backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1")] }));
  render(App);
  const main = await screen.findByLabelText("Review's main area");
  await waitFor(() => expect(shownReview()).toMatch(/release\/signoff/));
  const region = await within(main).findByLabelText("Human review");
  await within(main).findByRole("button", { name: "Claim to review (c)" });
  // Opened from the Pane's newest review, not from Verify: no way back; Find and Open in Verify.
  const header = main.querySelector("header")!;
  expect(within(header).getAllByRole("button").map((button) => button.getAttribute("aria-label") ?? button.textContent?.trim())).toEqual(["Find in the conversation", "Open in Verify"]);
  await fireEvent.click(within(header).getByRole("button", { name: "Find in the conversation" }));
  const find = within(header).getByRole("textbox", { name: "Find in the conversation" });
  await fireEvent.input(find, { target: { value: "release notes" } });
  await waitFor(() => expect(within(header).getByRole("status").textContent).toBe("1 of 1"));
  await fireEvent.keyDown(find, { key: "Escape" });
  expect(within(header).queryByRole("textbox", { name: "Find in the conversation" })).toBeNull();

  await fireEvent.keyDown(region, { key: "c" });
  await within(main).findByText("Yours");
  const reason = await within(main).findByRole("textbox", { name: "Reason" });
  reason.focus();
  // The first Esc leaves the reasons field with the review still open; the next returns to the Pane.
  await fireEvent.keyDown(reason, { key: "Escape" });
  await waitFor(() => expect(document.activeElement).toBe(region));
  expect(shownReview()).toMatch(/release\/signoff/);
  await fireEvent.keyDown(region, { key: "Escape" });
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Human reviews" })));
});

it("makes the claimed review shown in Review the active one, and shows the pet overlay while its evaluation space is not in front", async () => {
  const { calls } = backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1")] }));
  render(App);
  const main = await screen.findByLabelText("Review's main area");
  const region = await within(main).findByLabelText("Human review");
  await within(main).findByRole("button", { name: "Claim to review (c)" });
  const asked = (command: string) => calls.filter((call) => call.command === command).map((call) => (call.args as { request: string | null }).request);
  await waitFor(() => expect(asked("active_review_set")).toEqual(["run-a-2"]));
  await fireEvent.keyDown(region, { key: "c" });
  await within(main).findByText("Yours");
  expect(asked("active_review_set")).toEqual(["run-a-2"]);
  await fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
  // Neither opening nor taking the claim arms the pet: the backend event does.
  expect(asked("pet_show")).toEqual([]);
  await emit("active-review-changed", { request: "run-a-2" });
  await waitFor(() => expect(asked("pet_show")).toEqual(["run-a-2"]));
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  await waitFor(() => expect(asked("pet_show")).toEqual(["run-a-2", null]));
  // Another application in front: the overlay shows again.
  window.dispatchEvent(new Event("blur"));
  await waitFor(() => expect(asked("pet_show")).toEqual(["run-a-2", null, "run-a-2"]));
  window.dispatchEvent(new Event("focus"));
  await waitFor(() => expect(asked("pet_show")).toEqual(["run-a-2", null, "run-a-2", null]));
});

it("offers the graph's commands in the palette only while Verify is on screen", async () => {
  const main = branch("main", [runChip("run-0", "RED")]);
  backend(monitor(), () => browseView(1, { branches: [main] }));
  render(App);
  await fireEvent.click(await within(await screen.findByRole("group", { name: "Pane entries" })).findByRole("button", { name: /^app \/ main/ }));
  await waitFor(() => expect(shownRun()).toBe("run-0"));
  const offered = async () => {
    await fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    const labels = [...palette.querySelectorAll(".palette-item span:first-child")].map((item) => item.textContent);
    await fireEvent.keyDown(palette.querySelector("input")!, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull());
    return labels;
  };
  expect(await offered()).toContain("Open the Run's Detail");
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  const inReview = await offered();
  expect(inReview).not.toContain("Open the Run's Detail");
  expect(inReview).not.toContain("Find");
  expect(inReview).toContain("Show Verify");
  // New chat is offered from any mode.
  expect(inReview).toContain("New chat");
});

it("paints nothing of a mode that is not shown, its graph included, and keeps its place", async () => {
  const main = branch("main", [runChip("run-0", "RED")]);
  backend(monitor(), () => browseView(1, { branches: [main] }));
  render(App);
  await fireEvent.click(await within(await screen.findByRole("group", { name: "Pane entries" })).findByRole("button", { name: /^app \/ main/ }));
  await waitFor(() => expect(shownRun()).toBe("run-0"));
  await waitFor(() => expect(document.querySelector('main[aria-label="Verify"] .svelte-flow__node')).not.toBeNull());
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  const verify = workspace("Verify");
  expect(verify.hidden).toBe(true);
  // Transparent as a whole: its graph's nodes, which set their own visibility, and their layers
  // paint nothing over the mode shown.
  expect(getComputedStyle(verify).opacity).toBe("0");
  expect(verify.querySelector(".svelte-flow__node")).not.toBeNull();
  // Its place is kept: the Run is still shown when Verify comes back.
  await fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
  expect(shownRun()).toBe("run-0");
});

it("coalesces changes and reads Review again only when shown", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "setInterval", "clearTimeout", "clearInterval", "Date"] });
  try {
    let waiting = 1;
    const { calls } = backend(monitor(), () => browseView(1, { waiting: [waitingRow("run-0-1"), ...(waiting === 2 ? [waitingRow("run-0-2")] : [])] }));
    render(App);
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.getByRole("button", { name: "Review, 1 waiting" })).toBeTruthy();
    const count = (command: string) => calls.filter((call) => call.command === command).length;
    const [searched, counted] = [count("human_search"), count("human_waiting_count")];
    waiting = 2;
    await Promise.all(Array.from({ length: 30 }, () => emit("state-changed", { resync: false })));
    await vi.advanceTimersByTimeAsync(0);
    const review = screen.getByRole("button", { name: "Review, 2 waiting" });
    expect(count("human_search")).toBe(searched);
    expect(count("human_waiting_count") - counted).toBeGreaterThan(0);
    expect(count("human_waiting_count") - counted).toBeLessThanOrEqual(2);
    await fireEvent.click(review);
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.getAllByRole("option")).toHaveLength(2);
    expect(count("human_search")).toBe(searched + 1);
  } finally { vi.useRealTimers(); }
});

it("names the shown Run in the window's title while the Pane is hidden, and only the app while it is shown", async () => {
  const main = branch("main", [runChip("run-0", "RED")]);
  backend(monitor(), () => browseView(1, { branches: [main] }));
  windowTitles.length = 0;
  render(App);
  const titles = () => windowTitles;
  await waitFor(() => expect(titles().at(-1)).toBe("Artifactize"));
  await fireEvent.click(await within(await screen.findByRole("group", { name: "Pane entries" })).findByRole("button", { name: /^app \/ main/ }));
  await waitFor(() => expect(shownRun()).toBe("run-0"));
  // Ctrl+B hides the Pane: the title says what its selected row said.
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  await waitFor(() => expect(titles().at(-1)).toBe("run-0 · app / main — Artifactize"));
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  await waitFor(() => expect(titles().at(-1)).toBe("Artifactize"));
});
