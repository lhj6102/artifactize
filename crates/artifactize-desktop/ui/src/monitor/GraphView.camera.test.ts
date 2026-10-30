// The public camera-command seam: the viewport the graph asks Svelte Flow for, read against its
// canvas as a 1440×872 window lays it out. The graph's first view shows it whole; from then on
// only the person moves the camera. The Detail floats over the canvas and takes no part: Fit View
// fits the whole graph into the whole canvas, a Detail open or not. Opening what the find field
// found is the one exception the person asks for: it brings that Artifact to the middle of the
// graph left of the Detail.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { Viewport } from "@xyflow/svelte";
import type { Finding, Graph, Shape } from "../ipc/client";
import SourceGraph from "./SourceGraph.svelte";

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
  layout: async () => new Map(Object.entries(PLACED).map(([name, at]) => [`a:${name}`, at])),
}));

/** A box as getBoundingClientRect gives it. */
const box = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
/** At 1440×872: the graph's canvas right of the Pane, under the Run's headline. */
const CANVAS = box(346, 110, 1094, 762);
/** Where the Detail at its default width floats, 12px from the graph area's edges. */
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

/** Lay out the canvas and report every size change. */
function layOut(canvas: HTMLElement, width: number, height: number, at: DOMRect) {
  Object.defineProperty(canvas, "clientWidth", { configurable: true, value: width });
  Object.defineProperty(canvas, "clientHeight", { configurable: true, value: height });
  canvas.getBoundingClientRect = () => at;
  for (const observer of observers.filter((each) => each.live)) observer.callback([], {} as ResizeObserver);
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
const served = (shape: Shape): Graph => ({ source: { kind: "run", run: "run-a" }, shape: 1, update: { kind: "shape", ...shape } });

/** The Run's graph drawn and laid out; `find` answers the find field, given the text. */
async function drawn(find: (text: string) => Finding = () => ({ found: [], more: 0 })) {
  const graph = served({
    nodes: Object.keys(PLACED).map((name) => ({ id: `a:${name}`, name, light: "green" as const, note: null, parts: [] })),
    edges: [{ source: "a:release", target: "a:ship", light: "green" }],
  });
  mockIPC((command, payload) => {
    if (command === "graph") return graph;
    if (command === "find") return find((payload as { text: string }).text);
    return null;
  });
  const onopen = vi.fn();
  const shown = renderHosted(SourceGraph, { source: { kind: "run", run: "run-a" }, revision: 1, onopen, onclose: vi.fn() });
  await screen.findByLabelText("publish: GREEN");
  const canvas = document.querySelector<HTMLElement>(".canvas")!;
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  await tick();
  return { ...shown, canvas, onopen };
}

/** What `publish` matches: the Artifact itself, then its eval `reads`, and three more. */
const PUBLISH: Finding = {
  found: [
    { node: "a:publish", tab: null, artifact: "publish", eval: null, glyph: "✓", tone: "green" },
    { node: "a:publish", tab: "e:publish/reads", artifact: "publish", eval: "reads", glyph: "✗", tone: "red" },
  ],
  more: 3,
};

/** Open the find field as Ctrl+F does, and type `text` into it. */
async function typeFind(app: ReturnType<typeof renderHosted>["app"], text: string) {
  app.monitor.finding = true;
  const field = await screen.findByRole("combobox", { name: "Find in the graph" });
  expect(document.activeElement).toBe(field);
  await fireEvent.input(field, { target: { value: text } });
  return field;
}

it("shows the whole graph first, then leaves the camera as the Detail opens, changes width and closes", async () => {
  const { rerender, canvas } = await drawn();
  const names = Object.keys(PLACED);
  // Without a Detail the whole graph fills the canvas, publish where the Detail docks.
  expect(names.every((name) => within(name, CANVAS.right))).toBe(true);
  expect(onScreen("publish").right).toBeGreaterThan(DETAIL.left);
  const first = viewport;
  camera.setViewport.mockClear();

  // Opening publish's Detail, a new snapshot and closing it move nothing; the canvas keeps its
  // size under the floating Detail.
  await rerender({ opened: "a:publish" });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  await rerender({ revision: 2 });
  await rerender({ opened: null });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(camera.setViewport).not.toHaveBeenCalled();
  expect(viewport).toEqual(first);
});

it("keeps a camera the person moved, and fits the whole graph into the whole canvas only on Fit View", async () => {
  const { rerender, canvas } = await drawn();
  const first = viewport;
  // The person zoomed in towards the right: publish lies where the Detail docks.
  viewport = { x: -300, y: 40, zoom: 1.5 };
  const moved = viewport;
  camera.setViewport.mockClear();
  await rerender({ opened: "a:publish" });
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  // A neighbour opened from the Detail, out of view to the left, stays out of view.
  await rerender({ opened: "a:docs" });
  expect(camera.setViewport).not.toHaveBeenCalled();
  expect(viewport).toEqual(moved);
  expect(within("publish", DETAIL.left)).toBe(false);

  // Fit View, the person's own, shows every Artifact in the canvas, the same with the Detail
  // open as without: publish lies under the floating Detail again.
  screen.getByRole("button", { name: "Fit View" }).click();
  expect(Object.keys(PLACED).filter((name) => !within(name, CANVAS.right))).toEqual([]);
  expect(viewport).toEqual(first);
  expect(onScreen("publish").right).toBeGreaterThan(DETAIL.left);
});

it("fits a graph drawn in a hidden screen once its canvas first has a size", async () => {
  const graph = served({ nodes: [{ id: "a:docs", name: "docs", light: "green", note: null, parts: [] }], edges: [] });
  mockIPC((command) => command === "graph" ? graph : null);
  renderHosted(SourceGraph, { source: { kind: "run", run: "run-a" }, revision: 1, onopen: vi.fn(), onclose: vi.fn() });
  await screen.findByLabelText("docs: GREEN");
  const canvas = document.querySelector<HTMLElement>(".canvas")!;
  // Drawn hidden: the canvas had no size, so nothing was fitted yet.
  layOut(canvas, 0, 0, box(0, 0, 0, 0));
  expect(camera.setViewport).not.toHaveBeenCalled();
  layOut(canvas, CANVAS.width, CANVAS.height, CANVAS);
  expect(camera.setViewport).toHaveBeenCalledOnce();
  expect(within("docs", CANVAS.right)).toBe(true);
});

it("finds Artifacts and evals by name and brings the opened one to the middle of the graph left of the Detail", async () => {
  const asked: string[] = [];
  const { app, onopen } = await drawn((text) => (asked.push(text), text === "pub" ? PUBLISH : { found: [], more: 0 }));
  // The Detail of another Artifact is open, docked right.
  const detail = document.createElement("div");
  detail.tabIndex = -1;
  detail.getBoundingClientRect = () => DETAIL;
  document.body.append(detail);
  app.monitor.panes.detail = detail;
  // The person zoomed far out: names are too small to read.
  viewport = { x: 20, y: 30, zoom: 0.2 };
  camera.setViewport.mockClear();

  const field = await typeFind(app, "pub");
  const options = await screen.findAllByRole("option");
  expect(asked).toEqual(["pub"]);
  expect(options.map((option) => option.textContent)).toEqual(["✓publish", "✗publish / reads"]);
  expect(screen.getByRole("status").textContent?.trim()).toBe("3 more match.");
  // ↓ twice chooses the eval; Enter opens its Artifact's Detail on that eval's tab.
  await fireEvent.keyDown(field, { key: "ArrowDown" });
  await fireEvent.keyDown(field, { key: "ArrowDown" });
  expect(options[1].getAttribute("aria-selected")).toBe("true");
  await fireEvent.keyDown(field, { key: "Enter" });
  expect(onopen).toHaveBeenCalledExactlyOnceWith("a:publish", "e:publish/reads");
  await vi.waitFor(() => expect(camera.setViewport).toHaveBeenCalledOnce());
  expect(app.monitor.finding).toBe(false);
  expect(screen.queryByRole("combobox", { name: "Find in the graph" })).toBeNull();
  // publish now sits in the middle of the graph left of the Detail, zoomed in to read its name.
  expect(viewport.zoom).toBe(0.75);
  const at = onScreen("publish");
  expect((at.left + at.right) / 2).toBeCloseTo((CANVAS.left + DETAIL.left) / 2);
  expect((at.top + at.bottom) / 2).toBeCloseTo(CANVAS.top + CANVAS.height / 2);
  expect(document.activeElement).toBe(detail);
  detail.remove();
});

it("opens a found Artifact on its own tab with a click, keeping a zoom that reads", async () => {
  const { app, onopen } = await drawn(() => PUBLISH);
  viewport = { x: 0, y: 0, zoom: 1.5 };
  camera.setViewport.mockClear();
  await typeFind(app, "pub");
  const [own] = await screen.findAllByRole("option");
  // Pressing keeps the keyboard in the field and opens nothing; the click opens.
  expect(await fireEvent.mouseDown(own)).toBe(false);
  expect(onopen).not.toHaveBeenCalled();
  await fireEvent.click(own);
  expect(onopen).toHaveBeenCalledExactlyOnceWith("a:publish", null);
  await vi.waitFor(() => expect(camera.setViewport).toHaveBeenCalledOnce());
  // No Detail docked: the whole canvas is the graph's middle.
  expect(viewport.zoom).toBe(1.5);
  const at = onScreen("publish");
  expect((at.left + at.right) / 2).toBeCloseTo(CANVAS.left + CANVAS.width / 2);
});

it("says when nothing matches, and Esc closes the field leaving the graph and focus as they were", async () => {
  const { app, onopen } = await drawn();
  const before = document.createElement("button");
  document.body.append(before);
  before.focus();
  const kept = viewport;
  camera.setViewport.mockClear();
  const field = await typeFind(app, "nothing");
  expect((await screen.findByRole("status")).textContent?.trim()).toBe("No Artifact or eval matches.");
  expect(screen.queryAllByRole("option")).toEqual([]);
  await fireEvent.keyDown(field, { key: "Escape" });
  expect(app.monitor.finding).toBe(false);
  expect(screen.queryByRole("combobox", { name: "Find in the graph" })).toBeNull();
  expect(document.activeElement).toBe(before);
  expect(onopen).not.toHaveBeenCalled();
  expect(camera.setViewport).not.toHaveBeenCalled();
  expect(viewport).toEqual(kept);
  before.remove();
});
