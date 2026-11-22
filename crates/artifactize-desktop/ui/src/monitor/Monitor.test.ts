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
  selection: { backend: "fixture", model: "small", effort: null },
  createdAt: "2026-11-21T00:00:00Z", updatedAt: "2026-11-21T00:00:00Z", running: false, outcome: null, unseen: false,
} satisfies ChatSession;

/** The backend: `snapshot` answers every browse; the calls are recorded. */
function backend(launch: LaunchView, snapshot: () => BrowseView, steps: { view: (scan: number) => CurrentView } = { view: current }, outside: string | null = null, latest: ChatSession | null = saved) {
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
        case "chat_default_model":
          return saved.selection;
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
          if (step?.kind !== "open" && step?.kind !== "reference") return view;
          const live = view.branches.find((each) => each.chips.some((chip) => chip.kind === "run" && chip.id === step.id));
          return { ...view, run: { kind: "read", id: step.id, origin: null, node: `r:${step.id}`, mark: { status: "GREEN", glyph: "✓" }, needs: [], branch: live?.key ?? null } };
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
function workspace(mode: string): HTMLElement {
  const found = document.querySelector<HTMLElement>(`main[aria-label="${mode}"]`);
  if (!found) throw new Error(`no ${mode} workspace`);
  return found;
}

const monitor = (request: string | null = null): LaunchView => ({ mode: "monitor", state: "/state", reviewer: "alice", request });

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
  expect(screen.getByLabelText("Chat's main area").querySelector("h2")?.textContent).toBe("Work on main");
  await waitFor(() => expect(screen.getByLabelText("graph: would run (1 eval)")).toBeTruthy());
  expect(screen.getByLabelText("app: reused")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Graph" }).getAttribute("aria-pressed")).toBe("true");
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
  const header = () => screen.getByLabelText("Main area").querySelector("h2")?.textContent;
  // Collapsed, a branch shows its newest Run's status under its name.
  await waitFor(() => expect(name(/^app \/ feat\/x/).textContent).toMatch(/Running\s*5m/));
  expect(name(/^app \/ main/).textContent).toMatch(/RED\s*5m ago/);
  expect(screen.queryByRole("group", { name: /^Runs of/ })).toBeNull();
  expect(header()).toBe("Nothing chosen");
  // A click on a branch expands it and shows its newest entry: the Run in progress.
  await fireEvent.click(name(/^app \/ feat\/x/));
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-2" } }));
  expect(header()).toBe("run-2");
  const timeline = screen.getByRole("group", { name: "Runs of app feat/x" });
  // Each entry: its status and age, then its comment, or else its ID.
  expect(within(timeline).getAllByRole("button").map((entry) => entry.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "Running 5m Fixing the login form",
    "GREEN 5m ago run-1",
  ]);
  // ↓ enters the timeline, then steps to the older Run; ← goes back to the branch, ← again folds it.
  entries.focus();
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-1" } }));
  await fireEvent.keyDown(entries, { key: "ArrowLeft" });
  await fireEvent.keyDown(entries, { key: "ArrowLeft" });
  await waitFor(() => expect(screen.queryByRole("group", { name: "Runs of app feat/x" })).toBeNull());
  expect(header()).toBe("run-1");
  // ↓ to the next branch shows its newest Run; → expands it and → enters the timeline.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(header()).toBe("run-0"));
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
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-0"));
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
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-0");
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  expect(workspace("Review").hidden).toBe(false);
  expect(within(review).getByLabelText("Human review")).toBeTruthy();
});

it("says a Run's verdict and what needs attention in the header row, which opens the Run's Detail", async () => {
  const feature = branch("feat/x", [runChip("run-2", "YELLOW", false, "Fixing the login form")]);
  const run: RunPanel = {
    id: "run-2",
    branch: feature.key,
    origin: null,
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
  const main = screen.getByLabelText("Main area");
  const header = main.querySelector("header")!;
  const headline = await within(header).findByRole("button", { name: /open the Run's Detail/ });
  // One row: the Run's ID, then its verdict's icon and word, then each eval that needs attention.
  expect(within(header).getByRole("heading").textContent).toBe("run-2");
  expect([...headline.children].map((part) => part.textContent?.trim())).toEqual(["YELLOW", "cli/tests", "release/signoff"]);
  expect([...headline.querySelectorAll("svg")].map((icon) => icon.getAttribute("data-icon"))).toEqual(["yellow", "red", "human"]);
  // Its comment is the Run's Detail's and the timeline's, not the Main area's.
  expect(within(main).queryByText("Fixing the login form")).toBeNull();
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
  await fireEvent.click(screen.getByRole("button", { name: "Older runs" }));
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ older: main.key }));
  // At the oldest entry, ↓ asks again instead of leaving for the next branch.
  const asked = navigations().filter((step) => step.older).length;
  entries.focus();
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-1" } }));
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().filter((step) => step.older)).toHaveLength(asked + 1));
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-1");
});

it("opens the latest session in Graph from a Run header, and rescans when that branch's Run ends", async () => {
  let ended = false;
  let revision = 0;
  const main = () => branch("main", [ended ? runChip("run-1", "GREEN") : runChip("run-1", "RUNNING", true)]);
  const { calls, scanned } = backend(monitor(), () => browseView(++revision, { branches: [main()] }));
  render(App);
  await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
  const header = screen.getByLabelText("Main area").querySelector("header")!;
  await fireEvent.click(within(header).getByRole("button", { name: "Open in Chat" }));
  await waitFor(() => expect(scanned()).toBe(1));
  expect(calls.filter(({ command }) => command === "chat_latest_for_branch" || command === "chat_set_view"))
    .toEqual([{ command: "chat_latest_for_branch", args: { branch: main().key } }, { command: "chat_set_view", args: { id: saved.id, view: "graph" } }]);
  expect(workspace("Chat").hidden).toBe(false);
  expect(screen.getByRole("button", { name: "Graph" }).getAttribute("aria-pressed")).toBe("true");
  ended = true;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(scanned()).toBe(2));
});

it.each(["reference", "graph launch"])("opens New chat on the target branch when %s finds no session", async (entry) => {
  const other = branch("other", [runChip("run-other", "GREEN")]);
  const main = branch("main", []);
  const launched = entry === "graph launch";
  const { calls } = backend({ ...monitor(), mode: launched ? "graph" : "monitor" }, () => browseView(1, { branches: [other, main], launch: launched ? main.key : null }), { view: current }, null, null);
  render(App);
  if (!launched) {
    await fireEvent.click(await screen.findByRole("button", { name: /^app \/ main/ }));
    expect(screen.getByText("No Runs on this branch yet.")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Open in Chat" }));
  }
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  expect(workspace("Chat").hidden).toBe(false);
  expect(within(dialog).getByRole<HTMLSelectElement>("combobox", { name: "Branch" }).value).toBe(main.workspace);
  expect(calls.find(({ command }) => command === "chat_latest_for_branch")?.args).toEqual({ branch: main.key });
  expect(calls.find(({ command }) => command === "chat_default_model")?.args).toEqual({ worktree: main.workspace });
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
  // The review's eval names the main area, its title beside it.
  await waitFor(() => expect(main.querySelector("h2")?.textContent).toBe("release/signoff"));
  expect(workspace("Review").hidden).toBe(false);
  const rows = within(screen.getByRole("listbox", { name: "Human reviews" })).getAllByRole("option");
  expect(rows[0].getAttribute("aria-selected")).toBe("true");
  expect(within(main).getByLabelText("Human review")).toBeTruthy();
});

it("opens a review by reference without changing the filter or adding a banner, and leads back to Verify", async () => {
  const main = branch("main", [runChip("run-a", "GREEN")]);
  backend({ ...monitor("run-a-2"), mode: "review" }, () => browseView(1, { branches: [main] }), { view: current }, "claimed by you");
  render(App);
  const area = await screen.findByLabelText("Review's main area");
  await within(area).findByLabelText("Human review");
  expect(within(area).queryByText(/outside the filter/)).toBeNull();
  // The filter is left as it was.
  expect((screen.getByLabelText("Filter the reviews") as HTMLInputElement).value).toBe("type:human waiting");
  await fireEvent.click(within(area).getByRole("button", { name: "Verify · run-a › release" }));
  expect(workspace("Verify").hidden).toBe(false);
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-a"));
});

it("closes a review in Review with the one Close in its header, after leaving the review's form", async () => {
  backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1")] }));
  render(App);
  const main = await screen.findByLabelText("Review's main area");
  await waitFor(() => expect(main.querySelector("h2")?.textContent).toBe("release/signoff"));
  const region = await within(main).findByLabelText("Human review");
  await within(main).findByRole("button", { name: "Claim to review (c)" });
  // The way back to its Run leads the header; Close is its one control, and the review keeps
  // none of its own.
  const header = main.querySelector("h2")!.parentElement!;
  expect(within(header).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["Verify · run-a › release", "Close"]);
  expect(within(main).getAllByRole("button", { name: /Close/ })).toHaveLength(1);

  await fireEvent.keyDown(region, { key: "c" });
  await within(main).findByText("REVIEW (yours)");
  await fireEvent.keyDown(region, { key: "g" });
  await within(main).findByLabelText(/comment/);
  // The first Close leaves the form with the review still open; the next returns to the Pane.
  await fireEvent.click(within(header).getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(within(main).queryByLabelText(/comment/)).toBeNull());
  expect(main.querySelector("h2")?.textContent).toBe("release/signoff");
  await fireEvent.click(within(header).getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Human reviews" })));
});

it("makes the claimed review shown in Review the active one, and shows the pet overlay while its evaluation space is not in front", async () => {
  const { calls } = backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1")] }));
  render(App);
  const main = await screen.findByLabelText("Review's main area");
  const region = await within(main).findByLabelText("Human review");
  await within(main).findByRole("button", { name: "Claim to review (c)" });
  const asked = (command: string) => calls.filter((call) => call.command === command).map((call) => (call.args as { request: string | null }).request);
  expect(asked("active_review_set")).toEqual([]);
  await fireEvent.keyDown(region, { key: "c" });
  await waitFor(() => expect(asked("active_review_set")).toEqual(["run-a-2"]));
  // In front, the review needs no overlay.
  expect(asked("pet_show")).toEqual([]);
  await fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
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
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-0"));
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
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-0"));
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
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("run-0");
});

it("reads at most twice for a burst of changes, and nothing for a mode not on screen until it is shown", async () => {
  const { calls } = backend(monitor(), () => browseView(1, { waiting: [waitingRow("run-0-1")] }));
  render(App);
  await screen.findByRole("navigation", { name: "Modes" });
  // Settled after launch: count from here.
  await waitFor(() => expect(calls.some((call) => call.command === "human_search")).toBe(true));
  await new Promise((settle) => setTimeout(settle, 50));
  const count = (command: string) => calls.filter((call) => call.command === command).length;
  const [searched, counted] = [count("human_search"), count("human_waiting_count")];
  // A verify finishing: many changes at once.
  await Promise.all(Array.from({ length: 30 }, () => emit("state-changed", { resync: false })));
  await new Promise((settle) => setTimeout(settle, 50));
  // Review is not on screen: its list is not searched again; the bar's count reads at most twice.
  expect(count("human_search")).toBe(searched);
  expect(count("human_waiting_count") - counted).toBeLessThanOrEqual(2);
  // Shown, it reads once what changed meanwhile.
  await fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
  await waitFor(() => expect(count("human_search")).toBe(searched + 1));
  await new Promise((settle) => setTimeout(settle, 50));
  expect(count("human_search")).toBe(searched + 1);
});
