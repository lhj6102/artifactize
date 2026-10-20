// The public camera-command seam: the viewport the graph asks Svelte Flow for, read against the
// canvas and the Detail docked over its right side, as a 1440×872 window lays them out. A
// camera showing the whole graph fits it into what the Detail leaves; one the person moved keeps
// its zoom and only brings the opened Artifact out from under the Detail.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { Viewport } from "@xyflow/svelte";
import type { RunGraph as Graph, Shape } from "../ipc/client";
import RunGraph from "./RunGraph.svelte";

/** A layered Run: `publish` is the right-most column, where the Detail docks. */
const PLACED: Record<string, { x: number; y: number }> = {
  docs: { x: 0, y: 0 }, release: { x: 0, y: 84 }, ship: { x: 248, y: 84 },
  site: { x: 496, y: 0 }, notes: { x: 496, y: 168 }, publish: { x: 744, y: 84 },
};
const SIZE = { width: 168, height: 44 };

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
  layout: () => new Map(Object.entries(PLACED).map(([name, at]) => [`a:${name}`, at])),
}));

/** A box as getBoundingClientRect gives it. */
const box = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
/** At 1440×872: the graph's canvas right of Scope and Runs, under the Run's headline. */
const CANVAS = box(346, 110, 1094, 762);
/** The Detail at its default width, docked 12px from the graph area's edges. */
const DETAIL = box(925, 52, 503, 808);

/** The camera Svelte Flow would hold: the last viewport asked for, or one the person set. */
let viewport: Viewport = { x: 0, y: 0, zoom: 1 };
/** Every ResizeObserver the graph made, with what it observes, until it disconnects. */
let observers: { callback: ResizeObserverCallback; observed: Element[]; live: boolean }[] = [];

beforeAll(() => {
  window.matchMedia ??= ((query: string) => ({
    matches: false, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.DOMMatrixReadOnly ??= class {
    m22 = 1;
  } as unknown as typeof DOMMatrixReadOnly;
});

beforeEach(() => {
  observers = [];
  vi.stubGlobal("ResizeObserver", class {
    readonly entry = { callback: (() => {}) as ResizeObserverCallback, observed: [] as Element[], live: true };
    constructor(callback: ResizeObserverCallback) {
      this.entry.callback = callback;
      observers.push(this.entry);
    }
    observe(target: Element) {
      this.entry.observed.push(target);
    }
    disconnect() {
      this.entry.live = false;
    }
    unobserve() {}
  });
  viewport = { x: 0, y: 0, zoom: 1 };
  camera.getViewport.mockImplementation(() => viewport);
  camera.setViewport.mockImplementation(async (next: Viewport) => {
    viewport = next;
    return true;
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.resetAllMocks();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Lay out the canvas (and the Detail, when one is passed) and report every size change. */
function layOut(canvas: HTMLElement, width: number, height: number, at: DOMRect) {
  Object.defineProperty(canvas, "clientWidth", { configurable: true, value: width });
  Object.defineProperty(canvas, "clientHeight", { configurable: true, value: height });
  canvas.getBoundingClientRect = () => at;
  for (const observer of observers.filter((each) => each.live)) observer.callback([], {} as ResizeObserver);
}

/** A Detail element placed at `at`. */
function detailAt(at: DOMRect): HTMLElement {
  const aside = document.createElement("aside");
  aside.getBoundingClientRect = () => at;
  return aside;
}

/** Where the Artifact `name` shows in the window under the camera's viewport. */
function onScreen(name: string) {
  const { x, y } = PLACED[name];
  const left = CANVAS.left + x * viewport.zoom + viewport.x;
  const top = CANVAS.top + y * viewport.zoom + viewport.y;
  return { left, top, right: left + SIZE.width * viewport.zoom, bottom: top + SIZE.height * viewport.zoom };
}
const within = (name: string, right: number) => {
  const at = onScreen(name);
  return at.left >= CANVAS.left && at.right <= right && at.top >= CANVAS.top && at.bottom <= CANVAS.bottom;
};

/** Run A's graph as the backend first serves it: its whole shape. */
const served = (shape: Shape): Graph => ({ run: "run-a", shape: 1, update: { kind: "shape", ...shape } });

async function drawn() {
  const graph = served({
    nodes: Object.keys(PLACED).map((name) => ({ id: `a:${name}`, name, light: "green" as const })),
    edges: [{ source: "a:release", target: "a:ship", light: "green" }],
  });
  mockIPC((command) => command === "run_graph" ? graph : null);
  const shown = renderHosted(RunGraph, { run: "run-a", revision: 1, onopen: vi.fn(), onclose: vi.fn() });
  await screen.findByLabelText("publish: GREEN");
  const canvas = document.querySelector<HTMLElement>(".canvas")!;
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  await tick();
  return { ...shown, canvas };
}

it("fits the whole graph into what the Detail leaves as it opens, changes width and closes", async () => {
  const { rerender, canvas } = await drawn();
  const names = Object.keys(PLACED);
  // Without a Detail the whole graph fills the canvas, publish where the Detail docks.
  expect(names.every((name) => within(name, CANVAS.right))).toBe(true);
  expect(onScreen("publish").right).toBeGreaterThan(DETAIL.left);
  const full = viewport;

  // Opening publish's Detail fits every Artifact into the canvas left of it.
  const detail = detailAt(DETAIL);
  await rerender({ opened: "a:publish", cover: detail });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(names.filter((name) => !within(name, DETAIL.left))).toEqual([]);

  // A wider Detail fits them again, left of its new edge.
  const wider = box(700, 52, 728, 808);
  detail.getBoundingClientRect = () => wider;
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(names.filter((name) => !within(name, wider.left))).toEqual([]);

  // Closing it gives the graph the whole canvas again.
  await rerender({ opened: null, cover: null });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(viewport).toEqual(full);
});

it("keeps a camera the person moved, bringing only the opened Artifact out from under the Detail", async () => {
  const { rerender, canvas } = await drawn();
  // The person zoomed in towards the right: publish lies where the Detail docks.
  viewport = { x: -300, y: 40, zoom: 1.5 };
  expect(onScreen("publish").left).toBeGreaterThan(DETAIL.left);
  const detail = detailAt(DETAIL);
  await rerender({ opened: "a:publish", cover: detail });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(viewport.zoom).toBe(1.5);
  expect(within("publish", DETAIL.left)).toBe(true);

  // A neighbour opened from the Detail, out of view to the left, comes into view too.
  expect(onScreen("docs").left).toBeLessThan(CANVAS.left);
  await rerender({ opened: "a:docs" });
  expect(viewport.zoom).toBe(1.5);
  expect(within("docs", DETAIL.left)).toBe(true);

  // Closing the Detail leaves the person's camera where it is.
  camera.setViewport.mockClear();
  await rerender({ opened: null, cover: null });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(camera.setViewport).not.toHaveBeenCalled();
});

it("fits a graph drawn in a hidden screen once its canvas first has a size", async () => {
  const graph = served({ nodes: [{ id: "a:docs", name: "docs", light: "green" }], edges: [] });
  mockIPC((command) => command === "run_graph" ? graph : null);
  renderHosted(RunGraph, { run: "run-a", revision: 1, onopen: vi.fn(), onclose: vi.fn() });
  await screen.findByLabelText("docs: GREEN");
  const canvas = document.querySelector<HTMLElement>(".canvas")!;
  // Drawn hidden: the canvas had no size, so nothing was fitted yet.
  layOut(canvas, 0, 0, box(0, 0, 0, 0));
  expect(camera.setViewport).not.toHaveBeenCalled();
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(camera.setViewport).toHaveBeenCalledOnce();
  expect(within("docs", CANVAS.right)).toBe(true);
});
