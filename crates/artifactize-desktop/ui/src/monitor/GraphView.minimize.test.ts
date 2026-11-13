// A whole minimized and expanded on a Run's graph, against a mocked command boundary, layout
// and camera: its button draws it as a node that says how many parts it has, or as its box
// again, opening nothing; the camera keeps the whole's top left corner where it was on the
// screen; a minimized whole shows an Artifact it hides as opened; and the wholes minimized on a
// branch stay so on its other graphs.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import type { Viewport } from "@xyflow/svelte";
import type { BranchKey, Graph, Shape, Source } from "../ipc/client";
import SourceGraph from "./SourceGraph.svelte";

const camera = vi.hoisted(() => ({ fitView: vi.fn(), setViewport: vi.fn(), setCenter: vi.fn(), getViewport: vi.fn() }));
vi.mock("@xyflow/svelte", async (original) => {
  const flow = await original<typeof import("@xyflow/svelte")>();
  return { ...flow, useSvelteFlow: () => ({ ...flow.useSvelteFlow(), ...camera }) };
});
/** Each copy drawn as a node placed in a row, in drawing order, 240 apart, and the wholes each
 * layout was asked to minimize. */
const laid = vi.hoisted(() => ({ minimized: [] as string[][] }));
vi.mock("./layout", async () => {
  const { copies } = await import("./parts");
  return {
    NODE: { width: 168, height: 44 },
    layout: async (graph: Shape, minimized: readonly string[] = []) => {
      laid.minimized.push([...minimized]);
      const nodes = copies(graph, minimized).filter((copy) => copy.parts.length === 0);
      return new Map(nodes.map((copy, at) => [copy.key, { x: at * 240, y: 0 }]));
    },
  };
});

let viewport: Viewport = { x: 0, y: 0, zoom: 1 };

beforeAll(() => {
  window.matchMedia ??= ((query: string) => ({
    matches: false, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.DOMMatrixReadOnly ??= class {
    m22 = 1;
  } as unknown as typeof DOMMatrixReadOnly;
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

beforeEach(() => {
  laid.minimized = [];
  viewport = { x: 30, y: 20, zoom: 0.5 };
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
});

const BRANCH: BranchKey = { kind: "branch", common: "/work/app/.git", branch: "main" };

//   lib ──> app ──> site; suite ⊃ { app, site }
const SHAPE: Shape = {
  nodes: [
    { id: "a:lib", name: "lib", light: "green", note: null, parts: [] },
    { id: "a:suite", name: "suite", light: "red", note: null, parts: ["a:app", "a:site"] },
    { id: "a:app", name: "app", light: "red", note: null, parts: [] },
    { id: "a:site", name: "site", light: "green", note: null, parts: [] },
  ],
  edges: [
    { source: "a:lib", target: "a:app", light: "green" },
    { source: "a:app", target: "a:site", light: "red" },
  ],
};

/** Every Run serves the same shape. */
function serve() {
  mockIPC((command, payload) => {
    if (command !== "graph") return null;
    const { source } = payload as { source: Source };
    return { source, shape: 1, update: { kind: "shape", ...SHAPE } } satisfies Graph;
  });
}

const node = (id: string) => document.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);
const button = (name: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`);

async function drawn() {
  serve();
  const onopen = vi.fn();
  const view = renderHosted(SourceGraph, { source: { kind: "run", run: "run-a" }, branch: BRANCH, revision: 1, onopen, onclose: vi.fn() });
  await waitFor(() => expect(node("a:suite/a:app")).not.toBeNull());
  return { ...view, onopen };
}

it("minimizes a whole to a node that says how many parts it has and expands it again, opening nothing and keeping its corner on the screen", async () => {
  const { onopen } = await drawn();
  expect(node("a:suite")?.classList.contains("svelte-flow__node-whole")).toBe(true);
  // The box is around its parts: its corner is its first part's, less its room.
  const corner = (id: string) => {
    const at = node(id)!.style.transform.match(/translate\(([-\d.]+)px, ?([-\d.]+)px\)/)!;
    return { x: Number(at[1]) * viewport.zoom + viewport.x, y: Number(at[2]) * viewport.zoom + viewport.y };
  };
  const before = corner("a:suite");

  await fireEvent.click(button("Minimize suite")!);
  await waitFor(() => expect(node("a:suite")?.classList.contains("svelte-flow__node-minimized")).toBe(true));
  expect(onopen).not.toHaveBeenCalled();
  expect(laid.minimized.at(-1)).toEqual(["a:suite"]);
  expect(node("a:suite/a:app")).toBeNull();
  expect(node("a:suite")?.textContent).toContain("2 parts");
  expect(node("a:suite")?.getAttribute("aria-label")).toBe("suite: RED, minimized, 2 parts");
  await waitFor(() => expect(corner("a:suite")).toEqual(before));

  // A click on the minimized whole opens its Detail; Enter on its button only acts as the button.
  await fireEvent.click(node("a:suite")!);
  expect(onopen).toHaveBeenLastCalledWith("a:suite");
  button("Expand suite")!.focus();
  await fireEvent.keyDown(button("Expand suite")!, { key: "Enter" });
  expect(onopen).toHaveBeenCalledOnce();

  await fireEvent.click(button("Expand suite")!);
  await waitFor(() => expect(node("a:suite/a:app")).not.toBeNull());
  expect(node("a:suite")?.classList.contains("svelte-flow__node-whole")).toBe(true);
  expect(onopen).toHaveBeenCalledOnce();
  await waitFor(() => expect(corner("a:suite")).toEqual(before));
});

it("shows an Artifact a minimized whole hides as opened on that whole", async () => {
  const { rerender } = await drawn();
  await fireEvent.click(button("Minimize suite")!);
  await waitFor(() => expect(node("a:suite/a:app")).toBeNull());
  await rerender({ opened: "a:app" });
  expect(node("a:suite")?.querySelector(".open")).not.toBeNull();
  expect(node("a:lib")?.querySelector(".open")).toBeNull();
});

it("keeps a whole minimized on every graph of its branch, and only there", async () => {
  const { app, rerender } = await drawn();
  await fireEvent.click(button("Minimize suite")!);
  await waitFor(() => expect(node("a:suite/a:app")).toBeNull());
  expect(app.monitor.minimizedOn(BRANCH)).toEqual(["a:suite"]);

  // Another Run of the branch, and its Current, draw suite minimized.
  const before = laid.minimized.length;
  await rerender({ source: { kind: "run", run: "run-b" }, revision: 2 });
  // The new Run's own layout, not the one already drawn.
  await waitFor(() => expect(laid.minimized.length).toBe(before + 1));
  expect(laid.minimized.at(-1)).toEqual(["a:suite"]);
  await waitFor(() => expect(node("a:suite")?.classList.contains("svelte-flow__node-minimized")).toBe(true));
  await rerender({ source: { kind: "current", branch: BRANCH }, revision: 3 });
  await waitFor(() => expect(laid.minimized.length).toBe(3));
  expect(laid.minimized.at(-1)).toEqual(["a:suite"]);

  // Another branch's graph draws its box.
  const other: BranchKey = { kind: "branch", common: "/work/app/.git", branch: "feature" };
  await rerender({ source: { kind: "current", branch: other }, branch: other, revision: 4 });
  await waitFor(() => expect(laid.minimized.at(-1)).toEqual([]));
  await waitFor(() => expect(node("a:suite/a:app")).not.toBeNull());
});

it("minimizes and expands with m the whole focused on the graph, focus staying on it, as its button does", async () => {
  const { app, onopen } = await drawn();
  app.monitor.focus = "main";
  node("a:suite")!.focus();
  await fireEvent.keyDown(node("a:suite")!, { key: "m" });
  await waitFor(() => expect(node("a:suite")?.classList.contains("svelte-flow__node-minimized")).toBe(true));
  expect(app.monitor.minimizedOn(BRANCH)).toEqual(["a:suite"]);
  await waitFor(() => expect(document.activeElement).toBe(node("a:suite")));

  // On its Expand button, m expands it and focus stays on the button.
  button("Expand suite")!.focus();
  await fireEvent.keyDown(button("Expand suite")!, { key: "m" });
  await waitFor(() => expect(node("a:suite/a:app")).not.toBeNull());
  await waitFor(() => expect(document.activeElement).toBe(button("Minimize suite")));
  expect(onopen).not.toHaveBeenCalled();
});

it("minimizes and expands with m the whole whose Detail is open, and only a whole", async () => {
  const { app, rerender } = await drawn();
  app.monitor.focus = "detail";
  // A part's Detail: m does nothing.
  await rerender({ opened: "a:app" });
  await fireEvent.keyDown(document.body, { key: "m" });
  expect(app.monitor.minimizedOn(BRANCH)).toEqual([]);

  await rerender({ opened: "a:suite" });
  await fireEvent.keyDown(document.body, { key: "m" });
  await waitFor(() => expect(node("a:suite")?.classList.contains("svelte-flow__node-minimized")).toBe(true));
  await fireEvent.keyDown(document.body, { key: "m" });
  await waitFor(() => expect(node("a:suite")?.classList.contains("svelte-flow__node-whole")).toBe(true));
  expect(app.monitor.minimizedOn(BRANCH)).toEqual([]);
});

it("ignores m typed in a field, during a composition, with a modifier or with focus in the Pane", async () => {
  const { app, rerender } = await drawn();
  await rerender({ opened: "a:suite" });
  // Focus in the Pane.
  await fireEvent.keyDown(document.body, { key: "m" });
  app.monitor.focus = "detail";
  const field = document.createElement("input");
  document.body.append(field);
  await fireEvent.keyDown(field, { key: "m" });
  await fireEvent.keyDown(document.body, { key: "m", isComposing: true });
  await fireEvent.keyDown(document.body, { key: "m", ctrlKey: true });
  field.remove();
  expect(app.monitor.minimizedOn(BRANCH)).toEqual([]);
  expect(laid.minimized).toEqual([[]]);
});

it("names m on the graph's bottom and in the button's tooltip", async () => {
  await drawn();
  expect(document.querySelector(".hint")?.textContent).toContain("m minimizes or expands a whole");
  expect(button("Minimize suite")?.title).toBe("Minimize (m)");
});
