// The public camera-command seam has controlled completion: an old fit's shared
// completion must not interrupt a newer zoom-out transition at its intermediate zoom.
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

function pendingMove() {
  let resolve!: (moved: boolean) => void;
  const promise = new Promise<boolean>((done) => { resolve = done; });
  return { promise, resolve };
}

it("does not let an old fit completion interrupt the pending zoom-out camera move", async () => {
  const graph: Graph = {
    run: "run-a", nodes: [{ id: "a:app", name: "app", light: "green" }, { id: "a:other", name: "other", light: "green" }], edges: [],
  };
  const tree: TreeNode[] = [{
    id: "a:app", kind: "artifact", name: "app", glyph: "", tone: "green", weight: "normal",
    marks: "", text: [], right: "", changed: false, done: true, attention: false,
    urgent: false, upstream: [], children: [],
  }];
  mockIPC((command) => command === "run_graph" ? graph : null);
  const into = pendingMove();
  const whole = pendingMove();
  // fitView shares one pending resolver, whereas setViewport owns each transition's
  // completion. Let the older transition finish while the new one has not finished.
  camera.fitView.mockReturnValue(into.promise);
  camera.setViewport.mockReturnValueOnce(into.promise).mockReturnValueOnce(whole.promise);
  camera.setCenter.mockResolvedValue(true);
  const viewport = { x: 0, y: 0, zoom: 1.6 };
  camera.getViewport.mockImplementation(() => viewport);
  renderHosted(RunGraph, {
    run: "run-a", revision: 1, tree, selected: ["a:app"], onselect: vi.fn(), onopen: vi.fn(),
  });
  const node = await screen.findByLabelText("app: GREEN");
  const canvas = node.closest<HTMLElement>(".canvas")!;
  vi.spyOn(canvas, "clientWidth", "get").mockReturnValue(600);
  vi.spyOn(canvas, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 600, 400));
  // At the zoom-out endpoint the selected Artifact needs to be brought back into view.
  vi.spyOn(node, "getBoundingClientRect").mockReturnValue(new DOMRect(1000, 0, 168, 44));
  await fireEvent.click(node);
  const evals = await screen.findByRole("listbox", { name: "Evals" });
  expect(document.activeElement).toBe(evals);
  expect(camera.setViewport).toHaveBeenLastCalledWith(expect.objectContaining({ zoom: 1.6 }), { duration: 250 });
  await fireEvent.keyDown(evals, { key: "Escape" });
  expect(screen.queryByRole("listbox", { name: "Evals" })).toBeNull();
  expect(document.activeElement).toBe(node);
  expect(camera.setViewport).toHaveBeenLastCalledWith(expect.objectContaining({ zoom: 0.05 }), { duration: 250 });

  into.resolve(true);
  await into.promise;
  await tick();
  // Recentring now would use the intermediate zoom and interrupt the zoom-out animation.
  expect(camera.setCenter).not.toHaveBeenCalled();
  viewport.zoom = 0.05;
  whole.resolve(true);
  await whole.promise;
  await tick();
  expect(camera.setCenter).toHaveBeenLastCalledWith(84, 22, { zoom: 0.05, duration: 0 });
  expect(document.activeElement).toBe(node);
});
