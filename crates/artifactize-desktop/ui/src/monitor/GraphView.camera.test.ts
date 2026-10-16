// The public camera-command seam: an Artifact's eval popup opens and closes beside its node,
// and the graph's camera stays where the person left it.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { RunGraph as Graph, TreeNode } from "../ipc/client";
import RunGraph from "./RunGraph.svelte";

const camera = vi.hoisted(() => ({ fitView: vi.fn(), setViewport: vi.fn(), setCenter: vi.fn(), getViewport: vi.fn() }));
vi.mock("@xyflow/svelte", async (original) => {
  const flow = await original<typeof import("@xyflow/svelte")>();
  return {
    ...flow,
    useSvelteFlow: () => ({ ...flow.useSvelteFlow(), ...camera }),
  };
});
vi.mock("./layout", () => ({
  NODE: { width: 168, height: 44 },
  layout: async () => new Map([["a:app", { x: 0, y: 0 }], ["a:other", { x: 20000, y: 0 }]]),
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
  globalThis.DOMMatrixReadOnly ??= class {
    m22 = 1;
  } as unknown as typeof DOMMatrixReadOnly;
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

it("pops an Artifact's evals up and closes them without moving the camera", async () => {
  const graph: Graph = {
    run: "run-a", nodes: [{ id: "a:app", name: "app", light: "green" }, { id: "a:other", name: "other", light: "green" }], edges: [],
  };
  const tree: TreeNode[] = [{
    id: "a:app", kind: "artifact", name: "app", glyph: "", tone: "green", weight: "normal",
    marks: "", text: [], right: "", changed: false, done: true, attention: false,
    urgent: false, upstream: [], children: [],
  }];
  mockIPC((command) => command === "run_graph" ? graph : null);
  camera.getViewport.mockImplementation(() => ({ x: 0, y: 0, zoom: 1 }));
  renderHosted(RunGraph, {
    run: "run-a", revision: 1, tree, selected: ["a:app"], onselect: vi.fn(), onopen: vi.fn(),
  });
  const node = await screen.findByLabelText("app: GREEN");
  const canvas = node.closest<HTMLElement>(".canvas")!;
  vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 600, 400));
  vi.spyOn(node, "getBoundingClientRect").mockReturnValue(new DOMRect(20, 20, 168, 44));
  camera.fitView.mockClear();
  await fireEvent.click(node);
  const evals = await screen.findByRole("listbox", { name: "Evals" });
  expect(document.activeElement).toBe(evals);
  await fireEvent.keyDown(evals, { key: "Escape" });
  await tick();
  expect(screen.queryByRole("listbox", { name: "Evals" })).toBeNull();
  expect(document.activeElement).toBe(node);
  // The nodes stay where they were: neither the popup nor its closing moves the viewport.
  expect(camera.setViewport).not.toHaveBeenCalled();
  expect(camera.setCenter).not.toHaveBeenCalled();
  expect(camera.fitView).not.toHaveBeenCalled();
});
