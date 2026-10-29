// The window's one screen against a mocked command boundary: the Pane lists the branches the
// backend sends, expands them to their timelines and moves one selection through them and the
// Waiting stack; a timeline entry shows its
// Run or the branch's Current; a waiting review opens as Review; a Current scans again when
// chosen, refreshed or when a Run of its branch ends; the search's Esc puts sort:recent back.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { BrowseView, CurrentView, LaunchView, Navigate, RunPanel } from "../ipc/client";
import App from "../App.svelte";
import { branch, branchKey, browseView, runChip, waitingRow } from "./fixtures";
import { greenForm, humanView } from "../review/fixtures";

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

/** The structure text of the scan with this revision. */
const structure = (revision: number) => `app → graph; criteria revision ${revision}`;

/** The backend: `snapshot` answers every browse; the calls are recorded. */
function backend(launch: LaunchView, snapshot: () => BrowseView, steps: { view: (scan: number) => CurrentView } = { view: current }) {
  const calls: { command: string; args: unknown }[] = [];
  let scans = 0;
  /** The newest step the backend holds. */
  let shown: CurrentView | null = null;
  let stage: "claim" | "review" = "claim";
  mockIPC(
    (command, args) => {
      calls.push({ command, args });
      switch (command) {
        case "launch":
          return launch;
        case "browse":
        case "redraw":
          return snapshot();
        case "current_scan":
          shown = steps.view(++scans);
          return shown;
        case "current_view":
          return shown;
        case "current_structure":
          return structure(scans);
        case "graph":
          return {
            source: (args as { source: unknown }).source,
            shape: 1,
            update: {
              kind: "shape",
              nodes: [
                { id: "a:app", name: "app", light: "reused", note: "reused" },
                { id: "a:graph", name: "graph", light: "run", note: "would run (1 eval)" },
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
        case "human_form":
          return greenForm;
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

const monitor = (request: string | null = null): LaunchView => ({ mode: "monitor", state: "/state", reviewer: "alice", request });

it("opens on the Current of the branch it was started for, and scans it again on Refresh, Ctrl+R and F5", async () => {
  const main = branch("main", [{ kind: "current" }]);
  const { calls, scanned, navigations } = backend({ ...monitor(), mode: "graph" }, () => browseView(1, { branches: [main], launch: main.key }));
  render(App);
  /** The header names the time of the scan with this revision. */
  const header = async (revision: number) => {
    const status = await screen.findByText("Scanned at", { exact: false });
    await waitFor(() => expect(status.textContent?.trim()).toBe(`Scanned at ${new Date(scannedAt(revision)).toLocaleTimeString()}`));
    expect(status.querySelector("time")?.getAttribute("datetime")).toBe(new Date(scannedAt(revision)).toISOString());
  };
  await header(1);
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Current · app  main");
  await waitFor(() => expect(screen.getByLabelText("graph: would run (1 eval)")).toBeTruthy());
  expect(screen.getByLabelText("app: reused")).toBeTruthy();
  // The launched branch opens expanded; its Current is the entry shown.
  expect(screen.getByRole("button", { name: /^Current\b/ }).getAttribute("aria-pressed")).toBe("true");
  await fireEvent.click(screen.getByText(/Structure and evaluation criteria/));
  await waitFor(() => expect(screen.getByText(structure(1))).toBeTruthy());
  await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.getByText(structure(2))).toBeTruthy());
  await header(2);
  await fireEvent.keyDown(window, { key: "r", ctrlKey: true });
  await waitFor(() => expect(screen.getByText(structure(3))).toBeTruthy());
  await fireEvent.keyDown(window, { key: "F5" });
  await waitFor(() => expect(screen.getByText(structure(4))).toBeTruthy());
  expect(scanned()).toBe(4);
  // Only a person's refresh asks for the files as they are now, even past a scan still running.
  expect(calls.filter((call) => call.command === "current_scan").map((call) => (call.args as { refresh: boolean }).refresh))
    .toEqual([false, true, true, true]);
  // Each manual refresh also read the state again with every cache dropped.
  expect(navigations().filter((step) => step.resync)).toHaveLength(2);
});

it("draws a Current's graph at once, counting the Artifacts still being checked until its scan ends", async () => {
  const main = branch("main", [{ kind: "current" }]);
  const { publish } = backend({ ...monitor(), mode: "graph" }, () => browseView(1, { branches: [main], launch: main.key }), {
    view: (scan) => current(scan, 2),
  });
  render(App);
  await waitFor(() => expect(screen.getByText("Checking 2 of 2 Artifacts…")).toBeTruthy());
  await waitFor(() => expect(screen.getByLabelText("graph: would run (1 eval)")).toBeTruthy());
  expect(screen.getByRole("button", { name: "Refresh" }).hasAttribute("disabled")).toBe(true);
  await publish(current(2, 1));
  await waitFor(() => expect(screen.getByText("Checking 1 of 2 Artifacts…")).toBeTruthy());
  await publish({ ...current(3), failure: "Fingerprint script for Artifact app failed." });
  await waitFor(() => expect(screen.getByText("Fingerprint script for Artifact app failed.")).toBeTruthy());
  // The graph stays.
  expect(screen.getByLabelText("app: reused")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Refresh" }).hasAttribute("disabled")).toBe(false);
});

it("expands a clicked branch to its timeline, moves through timelines and the Waiting stack with the keys, and opens a review", async () => {
  const feature = branch("feat/x", [runChip("run-2", "RUNNING", true, "Fixing the login form"), runChip("run-1", "GREEN")]);
  const main = branch("main", [{ kind: "current" }, runChip("run-0", "RED")]);
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
  expect(header()).toBe("Run run-2");
  const timeline = screen.getByRole("group", { name: "Runs of app feat/x" });
  expect(within(timeline).getAllByRole("button").map((entry) => entry.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "Running 5m run-2 Fixing the login form",
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
  expect(header()).toBe("Run run-1");
  // ↓ to the next branch shows its newest entry, its Current; → expands it, → enters it, ↓ shows its Run.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(scanned()).toBe(1));
  expect(header()).toBe("Current · app  main");
  await fireEvent.keyDown(entries, { key: "ArrowRight" });
  expect(screen.getByRole("group", { name: "Runs of app main" })).toBeTruthy();
  await fireEvent.keyDown(entries, { key: "ArrowRight" });
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-0" } }));
  // ↓ again only selects the waiting review; Enter opens it as Review.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  const waiting = within(screen.getByRole("listbox", { name: "Waiting reviews" })).getByRole("option");
  expect(waiting.getAttribute("aria-selected")).toBe("true");
  expect(header()).toBe("Run run-0");
  await fireEvent.keyDown(entries, { key: "Enter" });
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Review"));
  expect(screen.getByRole("heading", { name: "Waiting (1)" })).toBeTruthy();
});

it("shows the comment a Run was requested with in full under its headline", async () => {
  const comment = "Fixing the login form so a wrong password says which field is wrong";
  const run: RunPanel = {
    id: "run-2",
    node: "run:run-2",
    mark: { status: "RUNNING", glyph: "◐" },
    headline: "validation pending · 5m",
    comment,
    counts: "◐1",
    attention: [],
    more: 0,
  };
  const feature = branch("feat/x", [runChip("run-2", "RUNNING", true, comment)]);
  backend(monitor(), () => browseView(1, { branches: [feature], run }));
  render(App);
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  await fireEvent.click(await within(entries).findByRole("button", { name: /^app \/ feat\/x/ }));
  const main = screen.getByLabelText("Main area");
  await waitFor(() => expect(within(main).getByText(comment).tagName).toBe("P"));
});

it("stars a branch from its entry, and takes a star away", async () => {
  const starred = branch("feat/x", [{ kind: "current" }], false, true);
  const main = branch("main", [{ kind: "current" }]);
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
  const main = branch("main", [{ kind: "current" }, runChip("run-1", "GREEN")], true);
  const later = branch("later", [{ kind: "current" }]);
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
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: { kind: "open", id: "run-1" } }));
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(navigations().filter((step) => step.older)).toHaveLength(asked + 1));
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Run run-1");
});

it("scans the shown Current again when a Run of its branch ends", async () => {
  let ended = false;
  let revision = 0;
  const snapshot = () =>
    browseView(++revision, {
      branches: [branch("main", ended ? [{ kind: "current" }, runChip("run-1", "GREEN")] : [runChip("run-1", "RUNNING", true)])],
    });
  const { scanned } = backend(monitor(), snapshot);
  render(App);
  // The Run in progress leads its timeline; once it ended, the Current leads it.
  await fireEvent.click(await screen.findByRole("button", { name: "Expand app / main" }));
  const current = () => screen.queryByRole("button", { name: /^Current\b/ });
  await waitFor(() => expect(screen.getByRole("group", { name: "Runs of app main" })).toBeTruthy());
  expect(current()).toBeNull();
  ended = true;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(current()).toBeTruthy());
  await fireEvent.click(current()!);
  await waitFor(() => expect(scanned()).toBe(1));
  // Another Run starts and ends on the branch: the Current is scanned again by itself.
  ended = false;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(current()).toBeNull());
  expect(scanned()).toBe(1);
  ended = true;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(scanned()).toBe(2));
});

it("searches the Pane as typed, and Esc puts sort:recent back before leaving the field", async () => {
  const { navigations } = backend(monitor(), () => browseView(1, { branches: [branch("main", [{ kind: "current" }])] }));
  render(App);
  const field = await screen.findByLabelText<HTMLInputElement>("Search branches and reviews");
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
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Review"));
  const rows = within(screen.getByRole("listbox", { name: "Waiting reviews" })).getAllByRole("option");
  expect(rows[0].getAttribute("aria-selected")).toBe("true");
});

it("closes a review in the Main area with the one Close in its header, after leaving the review's form", async () => {
  backend({ ...monitor(), mode: "review" }, () => browseView(1, { waiting: [waitingRow("run-9-1")] }));
  render(App);
  const main = await screen.findByLabelText("Main area");
  await waitFor(() => expect(main.querySelector("h2")?.textContent).toBe("Review"));
  const region = await within(main).findByLabelText("Human review");
  await within(main).findByRole("button", { name: "Claim to review" });
  // Close is the header's one control, and the review keeps none of its own.
  const header = main.querySelector("h2")!.parentElement!;
  expect(within(header).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["Close Esc"]);
  expect(within(main).getAllByRole("button", { name: /Close/ })).toHaveLength(1);

  await fireEvent.keyDown(region, { key: "c" });
  await within(main).findByText("REVIEW (yours)");
  await fireEvent.keyDown(region, { key: "g" });
  await within(main).findByLabelText(/comment/);
  // The first Close leaves the form with the review still open; the next returns to the Pane.
  await fireEvent.click(within(header).getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(within(main).queryByLabelText(/comment/)).toBeNull());
  expect(main.querySelector("h2")?.textContent).toBe("Review");
  await fireEvent.click(within(header).getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("group", { name: "Pane entries" })));
});
