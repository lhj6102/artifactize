// The Run graph against a mocked command boundary and layout: nodes take their lights, a click
// zooms into an Artifact and lists its evals from the tree, selecting one moves the tree's
// selection and Enter opens Detail, Esc zooms out, and a new snapshot recolours in place.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { RunGraph as Graph, TreeNode } from "../ipc/client";
import RunGraph from "./RunGraph.svelte";
import { LIMIT } from "./graph";

const layout = vi.hoisted(() => vi.fn());
vi.mock("./layout", () => ({ NODE: { width: 168, height: 44 }, layout }));

beforeAll(() => {
  // jsdom lays nothing out; the canvas only observes sizes and reads transforms.
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.DOMMatrixReadOnly ??= class {
    m22 = 1;
    constructor() {}
  } as unknown as typeof DOMMatrixReadOnly;
});

afterEach(() => {
  cleanup();
  clearMocks();
  layout.mockReset();
});

function graph(release: Graph["nodes"][number]["light"], ship = "waiting" as const): Graph {
  return {
    run: "run-a",
    nodes: [
      { id: "a:release", name: "release", light: release },
      { id: "a:ship", name: "ship", light: ship },
    ],
    edges: [{ source: "a:release", target: "a:ship", light: release, dependency: false }],
  };
}

const evalNode = (id: string, name: string, text: string): TreeNode => ({
  id,
  kind: "eval",
  glyph: "?",
  tone: "human",
  weight: "bold",
  name,
  marks: "",
  text: [{ text, tone: null }],
  right: "3s",
  changed: false,
  done: false,
  attention: true,
  urgent: true,
  upstream: [],
  children: [],
});

const tree: TreeNode[] = [
  {
    ...evalNode("a:release", "release", "in progress: signoff"),
    kind: "artifact",
    right: "0/2",
    children: [evalNode("e:release/signoff", "signoff", "Human sign-off · unclaimed"), evalNode("e:release/notes", "notes", "GREEN")],
  },
  { ...evalNode("a:ship", "ship", "waits for release"), kind: "artifact", children: [] },
];

function setup(served: () => Graph) {
  mockIPC((cmd) => (cmd === "run_graph" ? served() : null));
  layout.mockImplementation(async (laid: Graph) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
  const onselect = vi.fn();
  const onopen = vi.fn();
  const view = render(RunGraph, { run: "run-a", revision: 1, tree, selected: [], onselect, onopen });
  return { ...view, onselect, onopen };
}

const node = (id: string) => document.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);

describe("RunGraph", () => {
  it("lights nodes and edges, zooms into an Artifact's evals and selects and opens one", async () => {
    const { onselect, onopen } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    expect(node("a:release")?.dataset.light).toBe("active");
    expect(node("a:ship")?.dataset.light).toBe("waiting");
    expect(node("a:release")?.getAttribute("aria-label")).toBe("release: in progress");
    // Edges draw between measured handles, which jsdom never lays out; the e2e sees them.

    await fireEvent.click(node("a:release")!);
    const list = await screen.findByRole("listbox", { name: "Evals" });
    expect(screen.getByText("signoff")).not.toBeNull();
    expect(screen.getByText("Human sign-off · unclaimed")).not.toBeNull();
    expect(onselect).toHaveBeenLastCalledWith(["a:release", "e:release/signoff"]);
    await fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(onselect).toHaveBeenLastCalledWith(["a:release", "e:release/notes"]);
    await fireEvent.keyDown(list, { key: "Enter" });
    expect(onopen).toHaveBeenCalledOnce();
    await fireEvent.keyDown(list, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("listbox", { name: "Evals" })).toBeNull());
    expect(onselect).toHaveBeenLastCalledWith(["a:release"]);
  });

  it("moves along edges with arrows and zooms in with Enter on a focused node", async () => {
    const { onselect } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    node("a:release")!.focus();
    await fireEvent.keyDown(node("a:release")!, { key: "ArrowRight" });
    expect(onselect).toHaveBeenLastCalledWith(["a:ship"]);
    expect(document.activeElement).toBe(node("a:ship"));
    await fireEvent.keyDown(node("a:ship")!, { key: "Enter" });
    await screen.findByRole("listbox", { name: "Evals" });
    expect(screen.getByText("No evals.")).not.toBeNull();
  });

  it("recolours a new snapshot in place and lays out again only for a new shape", async () => {
    let served = graph("active");
    const { rerender } = setup(() => served);
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("active"));
    expect(layout).toHaveBeenCalledOnce();
    served = graph("green");
    await rerender({ revision: 2 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    expect(layout).toHaveBeenCalledOnce();
    served = { ...graph("green"), nodes: [...graph("green").nodes, { id: "a:docs", name: "docs", light: "waiting" }] };
    await rerender({ revision: 3 });
    await waitFor(() => expect(node("a:docs")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
  });

  it("never lets a slower layout of another Run replace the Run shown now", async () => {
    // Run A lays out at once; Run B's layout waits until the test releases it.
    const runs: Record<string, Graph> = {
      "run-a": graph("active"),
      "run-b": { ...graph("failed"), run: "run-b", nodes: [{ id: "a:other", name: "other", light: "failed" }], edges: [] },
    };
    mockIPC((cmd, args) => (cmd === "run_graph" ? runs[(args as { run: string }).run] : null));
    let release: (() => void) | null = null;
    layout.mockImplementation(async (laid: Graph) => {
      if (laid.run === "run-b") await new Promise<void>((done) => (release = done));
      return new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }]));
    });
    const props = { run: "run-a", revision: 1, tree, selected: [], onselect: vi.fn(), onopen: vi.fn() };
    const { rerender } = render(RunGraph, props);
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    await rerender({ run: "run-b", revision: 2 });
    await waitFor(() => expect(release).not.toBeNull());
    await rerender({ run: "run-a", revision: 3 });
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    release!();
    await new Promise((done) => setTimeout(done, 50));
    expect(node("a:other")).toBeNull();
    expect(node("a:release")?.dataset.light).toBe("active");
    expect(layout).toHaveBeenCalledTimes(2);
  });

  it("opens the highlighted eval after the tree's selection left it on a live completion", async () => {
    let served = graph("active");
    const { rerender, onselect, onopen } = setup(() => served);
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    await fireEvent.click(node("a:release")!);
    const list = await screen.findByRole("listbox", { name: "Evals" });
    await rerender({ selected: ["a:release", "e:release/signoff"] });
    // The sign-off completes elsewhere: the tree folds the done Artifact and selects it.
    served = graph("green");
    await rerender({ revision: 2, selected: ["a:release"] });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    expect(screen.getByRole("option", { selected: true }).textContent).toContain("signoff");
    onselect.mockClear();
    await fireEvent.keyDown(list, { key: "Enter" });
    expect(onselect).toHaveBeenCalledWith(["a:release", "e:release/signoff"]);
    expect(onselect.mock.invocationCallOrder[0]).toBeLessThan(onopen.mock.invocationCallOrder[0]);
  });

  it("asks before laying out more than the limit, once per Run", async () => {
    let size = LIMIT + 1;
    const many = (run: string): Graph => ({
      run,
      nodes: Array.from({ length: size }, (_, index) => ({ id: `a:${run}-${index}`, name: `${run}-${index}`, light: "green" })),
      edges: [],
    });
    mockIPC((cmd, args) => (cmd === "run_graph" ? many((args as { run: string }).run) : null));
    layout.mockImplementation(async (laid: Graph) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
    const props = { run: "run-a", revision: 1, tree, selected: [], onselect: vi.fn(), onopen: vi.fn() };
    const { rerender } = render(RunGraph, props);
    await screen.findByText(new RegExp(`has ${LIMIT + 1} Artifacts`));
    expect(layout).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole("button", { name: `Lay out all ${LIMIT + 1}` }));
    await waitFor(() => expect(node("a:run-a-0")).not.toBeNull());
    expect(layout).toHaveBeenCalledOnce();
    // A new snapshot of the same Run with another Artifact (a new shape, still above the
    // limit) keeps the answer and lays out again; another Run asks again.
    size = LIMIT + 2;
    await rerender({ revision: 2 });
    await waitFor(() => expect(node(`a:run-a-${LIMIT + 1}`)).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: /Lay out all/ })).toBeNull();
    await rerender({ run: "run-b", revision: 3 });
    await screen.findByRole("button", { name: `Lay out all ${LIMIT + 2}` });
    expect(layout).toHaveBeenCalledTimes(2);
  });

  it("lays out again after a failed layout, and after the layout cache dropped a Run", async () => {
    const small = (run: string): Graph => ({ ...graph("green"), run });
    mockIPC((cmd, args) => (cmd === "run_graph" ? small((args as { run: string }).run) : null));
    let failing = true;
    layout.mockImplementation(async (laid: Graph) => {
      if (failing) throw new Error("layout failed");
      return new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }]));
    });
    const props = { run: "run-0", revision: 1, tree, selected: [], onselect: vi.fn(), onopen: vi.fn() };
    const { rerender } = render(RunGraph, props);
    await screen.findByText("layout failed");
    // A failed layout is not kept: the next snapshot tries again.
    failing = false;
    await rerender({ revision: 2 });
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
    // Eight more Runs push run-0's layout out of the cache; back on run-0 it is laid out again.
    let revision = 3;
    for (let run = 1; run <= 8; run += 1) {
      await rerender({ run: `run-${run}`, revision: revision++ });
      await waitFor(() => expect(layout).toHaveBeenCalledTimes(2 + run));
    }
    await rerender({ run: "run-8", revision: revision++ });
    await rerender({ run: "run-1", revision: revision++ });
    await new Promise((done) => setTimeout(done, 20));
    expect(layout).toHaveBeenCalledTimes(10);
    await rerender({ run: "run-0", revision: revision++ });
    await waitFor(() => expect(layout).toHaveBeenCalledTimes(11));
  });

  it("lets go of keyboard focus when its screen hides and does not take it back unasked", async () => {
    const { rerender } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    node("a:release")!.focus();
    expect(document.activeElement).toBe(node("a:release"));
    const frames = vi.spyOn(window, "requestAnimationFrame");
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    const settle = () => new Promise((done) => setTimeout(done, 120));
    const nodeFocus = () => focus.mock.contexts.filter((element) => (element as HTMLElement).classList.contains("svelte-flow__node")).length;

    // Another screen shows an already open Detail: nothing there takes focus.
    await rerender({ active: false });
    (document.activeElement as HTMLElement).blur();
    await settle();
    const called = frames.mock.calls.length;
    await settle();
    expect(frames.mock.calls.length).toBe(called);
    expect(nodeFocus()).toBe(0);
    expect(document.activeElement).toBe(document.body);

    // Back on the graph's screen: no hold returns until a node is focused afresh.
    await rerender({ active: true });
    await settle();
    expect(nodeFocus()).toBe(0);
    expect(document.activeElement).toBe(document.body);

    // A hidden ancestor (the screen's hidden attribute) ends a fresh hold the same way.
    node("a:release")!.focus();
    const screenElement = node("a:release")!.closest(".graph")!.parentElement!;
    screenElement.setAttribute("hidden", "");
    (document.activeElement as HTMLElement).blur();
    focus.mockClear();
    await settle();
    const hiddenCalls = frames.mock.calls.length;
    await settle();
    expect(frames.mock.calls.length).toBe(hiddenCalls);
    expect(nodeFocus()).toBe(0);
    screenElement.removeAttribute("hidden");
    frames.mockRestore();
    focus.mockRestore();
  });
});
