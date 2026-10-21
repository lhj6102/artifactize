// The window's one screen against a mocked command boundary: the Pane lists the branches the
// backend sends and moves one selection through them and the Waiting stack; a chip shows its
// Run or the branch's Current; a waiting review opens as Review; a Current scans again when
// chosen, refreshed or when a Run of its branch ends; the search's Esc puts sort:recent back.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { BrowseView, CurrentView, LaunchView, Navigate } from "../ipc/client";
import App from "../App.svelte";
import { branch, branchKey, browseView, runChip, waitingRow } from "./fixtures";
import { humanView } from "../review/fixtures";

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

const current = (revision: number): CurrentView => ({
  branch: branchKey("main"),
  scannedAt: scannedAt(revision),
  fingerprint: `graph:revision-${revision}`,
  structure: `app → graph; criteria revision ${revision}`,
  tree: [],
  revision,
});

/** The backend: `snapshot` answers every browse; the calls are recorded. */
function backend(launch: LaunchView, snapshot: () => BrowseView) {
  const calls: { command: string; args: unknown }[] = [];
  let scans = 0;
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
          return current(++scans);
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
          return humanView("claim");
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  const navigations = () => calls.filter((call) => call.command === "browse").map((call) => (call.args as { navigate: Navigate }).navigate);
  const scanned = () => calls.filter((call) => call.command === "current_scan").length;
  return { calls, navigations, scanned };
}

const monitor = (request: string | null = null): LaunchView => ({ mode: "monitor", state: "/state", reviewer: "alice", request });

it("opens on the Current of the branch it was started for, and scans it again on Refresh, Ctrl+R and F5", async () => {
  const main = branch("main", [{ kind: "current" }]);
  const { scanned, navigations } = backend({ ...monitor(), mode: "graph" }, () => browseView(1, { branches: [main], launch: main.key }));
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
  // The Current chip is the one shown, filled.
  expect(screen.getByRole("button", { name: "Current" }).getAttribute("aria-pressed")).toBe("true");
  await fireEvent.click(screen.getByText(/Structure and evaluation criteria/));
  expect(screen.getByText("app → graph; criteria revision 1")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 2")).toBeTruthy());
  await header(2);
  await fireEvent.keyDown(window, { key: "r", ctrlKey: true });
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 3")).toBeTruthy());
  await fireEvent.keyDown(window, { key: "F5" });
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 4")).toBeTruthy());
  expect(scanned()).toBe(4);
  // Each manual refresh also read the state again with every cache dropped.
  expect(navigations().filter((step) => step.resync)).toHaveLength(2);
});

it("selects a branch's last chip, steps through its chips and the Waiting stack with the keys, and opens a review", async () => {
  const feature = branch("feat/x", [runChip("run-1", "GREEN"), runChip("run-2", "RUNNING", true)]);
  const main = branch("main", [runChip("run-0", "RED"), { kind: "current" }]);
  const { navigations, scanned } = backend(monitor(), () => browseView(1, { branches: [feature, main], waiting: [waitingRow("run-0-1")] }));
  render(App);
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  await waitFor(() => expect(within(entries).getByText(/^app\s+feat\/x$/)).toBeTruthy());
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Nothing chosen");
  // A click on a branch's name shows its last chip: the Run in progress.
  await fireEvent.click(within(entries).getByText(/^app\s+feat\/x$/));
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: "run-2" }));
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Run run-2");
  // ← steps to the older chip; ↓ to the next branch, whose last chip is its Current.
  entries.focus();
  await fireEvent.keyDown(entries, { key: "ArrowLeft" });
  await waitFor(() => expect(navigations().at(-1)).toMatchObject({ run: "run-1" }));
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  await waitFor(() => expect(scanned()).toBe(1));
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Current · app  main");
  // ↓ again only selects the waiting review; Enter opens it as Review.
  await fireEvent.keyDown(entries, { key: "ArrowDown" });
  const waiting = within(screen.getByRole("listbox", { name: "Waiting reviews" })).getByRole("option");
  expect(waiting.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Current · app  main");
  await fireEvent.keyDown(entries, { key: "Enter" });
  await waitFor(() => expect(screen.getByLabelText("Main area").querySelector("h2")?.textContent).toBe("Review"));
  expect(screen.getByRole("heading", { name: "Waiting (1)" })).toBeTruthy();
});

it("scans the shown Current again when a Run of its branch ends", async () => {
  let ended = false;
  let revision = 0;
  const snapshot = () =>
    browseView(++revision, {
      branches: [branch("main", ended ? [runChip("run-1", "GREEN"), { kind: "current" }] : [runChip("run-1", "RUNNING", true)])],
    });
  const { scanned } = backend(monitor(), snapshot);
  render(App);
  // The Run in progress ends its row; once it ended, the row ends with the Current chip.
  const entries = await screen.findByRole("group", { name: "Pane entries" });
  await waitFor(() => expect(within(entries).getByText(/^app\s+main$/)).toBeTruthy());
  ended = true;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(screen.getByRole("button", { name: "Current" })).toBeTruthy());
  await fireEvent.click(screen.getByRole("button", { name: "Current" }));
  await waitFor(() => expect(scanned()).toBe(1));
  // Another Run starts and ends on the branch: the Current is scanned again by itself.
  ended = false;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(screen.queryByRole("button", { name: "Current" })).toBeNull());
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
