// The public camera-command seam: a click on an Artifact asks for its Detail and one on the
// background closes it, and the graph's camera stays where the person left it.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { RunGraph as Graph } from "../ipc/client";
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

it("opens and closes an Artifact's Detail without moving the camera", async () => {
  const graph: Graph = {
    run: "run-a", nodes: [{ id: "a:app", name: "app", light: "green" }, { id: "a:other", name: "other", light: "green" }], edges: [],
  };
  mockIPC((command) => command === "run_graph" ? graph : null);
  camera.getViewport.mockImplementation(() => ({ x: 0, y: 0, zoom: 1 }));
  const onopen = vi.fn();
  const onclose = vi.fn();
  const { rerender } = renderHosted(RunGraph, { run: "run-a", revision: 1, onopen, onclose });
  const node = await screen.findByLabelText("app: GREEN");
  camera.fitView.mockClear();
  await fireEvent.click(node);
  expect(onopen).toHaveBeenCalledWith("a:app");
  await rerender({ opened: "a:app" });
  await fireEvent.click(document.querySelector<HTMLElement>(".svelte-flow__pane")!);
  expect(onclose).toHaveBeenCalledOnce();
  await rerender({ opened: null });
  await tick();
  // The nodes stay where they were: neither opening nor closing a Detail moves the viewport.
  expect(camera.setViewport).not.toHaveBeenCalled();
  expect(camera.setCenter).not.toHaveBeenCalled();
  expect(camera.fitView).not.toHaveBeenCalled();
});
