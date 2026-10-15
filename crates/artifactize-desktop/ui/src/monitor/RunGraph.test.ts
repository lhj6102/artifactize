import { renderHosted as render } from "../test-host";
// The Run graph against a mocked command boundary and layout: nodes take their lights, a click
// zooms into an Artifact and lists its evals from the tree, selecting one moves the tree's
// selection and Enter opens Detail, Esc zooms out, and a new snapshot recolours in place.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
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
  vi.useRealTimers();
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
      "run-b": { ...graph("red"), run: "run-b", nodes: [{ id: "a:other", name: "other", light: "red" }], edges: [] },
    };
    mockIPC((cmd, args) => (cmd === "run_graph" ? runs[(args as { run: string }).run] : null));
    let release!: (positions: Map<string, { x: number; y: number }>) => void;
    const staleLayout = new Promise<Map<string, { x: number; y: number }>>((done) => (release = done));
    let started!: () => void;
    const layingOut = new Promise<void>((done) => (started = done));
    layout.mockImplementation((laid: Graph) => {
      if (laid.run === "run-b") {
        started();
        return staleLayout;
      }
      return Promise.resolve(new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
    });
    const props = { run: "run-a", revision: 1, tree, selected: [], onselect: vi.fn(), onopen: vi.fn() };
    const { rerender } = render(RunGraph, props);
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    await rerender({ run: "run-b", revision: 2 });
    await layingOut;
    runs["run-a"] = graph("green");
    await rerender({ run: "run-a", revision: 3 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    release(new Map([["a:other", { x: 0, y: 0 }]]));
    // place() already awaits this exact layout promise: let its continuation run, then
    // flush any view update before asserting that the stale response did not replace Run A.
    await staleLayout;
    await tick();
    expect(node("a:other")).toBeNull();
    expect(node("a:release")?.dataset.light).toBe("green");
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
    const small = (run: string): Graph => ({
      run,
      nodes: [{ id: `a:${run}`, name: run, light: "green" }],
      edges: [],
    });
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
    await waitFor(() => expect(node("a:run-0")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
    // Eight more Runs push run-0's layout out of the cache; back on run-0 it is laid out again.
    let revision = 3;
    for (let run = 1; run <= 8; run += 1) {
      await rerender({ run: `run-${run}`, revision: revision++ });
      await waitFor(() => expect(node(`a:run-${run}`)).not.toBeNull());
    }
    await rerender({ run: "run-8", revision: revision++ });
    await rerender({ run: "run-1", revision: revision++ });
    await waitFor(() => expect(node("a:run-1")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(10);
    await rerender({ run: "run-0", revision: revision++ });
    await waitFor(() => expect(node("a:run-0")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(11);
  });

  it("lets go of keyboard focus when its screen hides and does not take it back unasked", async () => {
    const { rerender } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    // Start the controlled frame clock before focus schedules its first watch.
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const frame = async () => {
      vi.advanceTimersToNextFrame();
      await tick();
    };
    node("a:release")!.focus();
    expect(document.activeElement).toBe(node("a:release"));

    // Another screen shows an already open Detail: nothing there takes focus.
    await rerender({ active: false });
    (document.activeElement as HTMLElement).blur();
    await frame();
    expect(document.activeElement).toBe(document.body);
    await frame();
    expect(document.activeElement).toBe(document.body);

    // Back on the graph's screen: no hold returns until a node is focused afresh.
    await rerender({ active: true });
    await frame();
    expect(document.activeElement).toBe(document.body);

    // A hidden ancestor (the screen's hidden attribute) ends a fresh hold the same way.
    node("a:release")!.focus();
    const screenElement = node("a:release")!.closest(".graph")!.parentElement!;
    screenElement.setAttribute("hidden", "");
    (document.activeElement as HTMLElement).blur();
    await frame();
    expect(document.activeElement).toBe(document.body);
    await frame();
    expect(document.activeElement).toBe(document.body);
    screenElement.removeAttribute("hidden");
    await frame();
    expect(document.activeElement).toBe(document.body);
  });

  it("keeps a key that lands on the page while a redraw drops the held node's focus", async () => {
    const { rerender, onselect } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    node("a:release")!.focus();
    onselect.mockClear();
    // The redraw's gap: focus is on the page when the key arrives, before any frame.
    (document.activeElement as HTMLElement).blur();
    await fireEvent.keyDown(document.body, { key: "ArrowRight" });
    expect(onselect).toHaveBeenCalledWith(["a:ship"]);
    vi.advanceTimersToNextFrame();
    await tick();
    expect(document.activeElement).toBe(node("a:ship"));

    // A hidden graph's hold is gone: a key on the page is not the graph's.
    await rerender({ active: false });
    (document.activeElement as HTMLElement).blur();
    onselect.mockClear();
    await fireEvent.keyDown(document.body, { key: "ArrowLeft" });
    expect(onselect).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(document.body);
  });

  it("puts focus back as soon as a redraw removes the focused node", async () => {
    setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    node("a:release")!.focus();
    // Culling takes the node out and draws it again in one update.
    const element = node("a:release")!;
    const parent = element.parentElement!;
    const next = element.nextSibling;
    element.remove();
    parent.insertBefore(element, next);
    // Before any frame or key: the observer has run by the next microtask checkpoint.
    await Promise.resolve();
    expect(document.activeElement).toBe(element);
  });

  it("shows ERROR apart from RED, and a legend of all six lights", async () => {
    setup(() => ({
      run: "run-a",
      nodes: [
        { id: "a:release", name: "release", light: "red" },
        { id: "a:ship", name: "ship", light: "error" },
        { id: "a:docs", name: "docs", light: "blocked" },
      ],
      edges: [{ source: "a:ship", target: "a:docs", light: "error", dependency: false }],
    }));
    await waitFor(() => expect(node("a:ship")).not.toBeNull());
    expect(node("a:release")?.getAttribute("aria-label")).toBe("release: RED");
    expect(node("a:ship")?.getAttribute("aria-label")).toBe("ship: ERROR");
    expect(node("a:ship")?.querySelector(".light-error")).not.toBeNull();
    expect(node("a:docs")?.getAttribute("aria-label")).toBe("docs: blocked by upstream");
    const legend = [...screen.getByRole("list", { name: "Legend" }).querySelectorAll("li")];
    expect(legend.map((item) => item.className.split(" ").find((name) => name.startsWith("light-")))).toEqual([
      "light-green",
      "light-active",
      "light-waiting",
      "light-red",
      "light-error",
      "light-blocked",
    ]);
  });
});
