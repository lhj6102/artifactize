// The current-graph tool screen renders the backend extraction through the existing graph
// view, with no Run lookup; refresh replaces its criteria and fingerprint together.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { CurrentGraph as View } from "../ipc/client";
import App from "../App.svelte";

vi.mock("./layout", () => ({
  NODE: { width: 168, height: 44 },
  layout: async () => new Map([["a:app", { x: 0, y: 0 }], ["a:graph", { x: 240, y: 0 }]]),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }),
}));

beforeAll(() => {
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

afterEach(() => { cleanup(); clearMocks(); });

it("shows the current extraction without a Run and reloads it through the shared command", async () => {
  let revision = 1;
  const calls: string[] = [];
  mockIPC((command) => {
    if (command === "launch") return { mode: "graph", state: "scratch", reviewer: "alice", search: "", request: null };
    if (command === "browse" || command === "redraw") return {
      scope: { kind: "all" }, scopeLabel: "all repositories", scopes: [], badge: [], runs: [],
      selectedRun: null, older: false, newer: false, run: null, error: null, refreshed: true, replaced: false, revision: 1,
    };
    if (command.startsWith("plugin:event|")) return 0;
    if (command !== "current_graph") throw new Error(`Unexpected command: ${command}`);
    calls.push(command);
    const view: View = {
      fingerprint: `graph:revision-${revision}`,
      graph: {
        nodes: [{ id: "a:app", name: "app", light: "waiting" }, { id: "a:graph", name: "graph", light: "waiting" }],
        edges: [{ source: "a:app", target: "a:graph", light: "waiting", dependency: false }],
      },
      tree: [],
      structure: `app → graph; criteria revision ${revision}`,
    };
    return view;
  });
  // Mount the real shell first: its window handler precedes the graph screen's handler.
  render(App);
  await waitFor(() => expect(screen.getByText("app")).toBeTruthy());
  expect(screen.getByText("graph")).toBeTruthy();
  expect(screen.getByText("Current structure · arrows: input → consumer")).toBeTruthy();
  await fireEvent.click(screen.getByText(/Structure and evaluation criteria/));
  expect(screen.getByText("app → graph; criteria revision 1")).toBeTruthy();
  revision = 2;
  await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 2")).toBeTruthy());
  expect(screen.getByText(/graph:revision-2/)).toBeTruthy();
  revision = 3;
  await fireEvent.keyDown(window, { key: "r", ctrlKey: true });
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 3")).toBeTruthy());
  expect(screen.getByText(/graph:revision-3/)).toBeTruthy();
  revision = 4;
  await fireEvent.keyDown(window, { key: "F5" });
  await waitFor(() => expect(screen.getByText("app → graph; criteria revision 4")).toBeTruthy());
  expect(screen.getByText(/graph:revision-4/)).toBeTruthy();
  expect(calls).toEqual(["current_graph", "current_graph", "current_graph", "current_graph"]);
});
