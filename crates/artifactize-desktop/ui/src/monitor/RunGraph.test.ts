import { renderHosted as render } from "../test-host";
// The Run graph against a mocked command boundary and layout: nodes take their lights, a click
// (or Enter on a focused node) on an Artifact asks for its Detail and one on the background
// closes it, and a new snapshot recolours in place.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { RunGraph as Graph } from "../ipc/client";
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
    edges: [{ source: "a:release", target: "a:ship", light: release }],
  };
}

function setup(served: () => Graph) {
  mockIPC((cmd) => (cmd === "run_graph" ? served() : null));
  layout.mockImplementation(async (laid: Graph) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
  const onopen = vi.fn();
  const onclose = vi.fn();
  const view = render(RunGraph, { run: "run-a", revision: 1, onopen, onclose });
  return { ...view, onopen, onclose };
}

const node = (id: string) => document.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);

describe("RunGraph", () => {
  it("lights nodes and edges, and asks for an Artifact's Detail on a click or Enter and closes it on the background", async () => {
    const { onopen, onclose, rerender } = setup(() => graph("active"));
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    expect(node("a:release")?.dataset.light).toBe("active");
    expect(node("a:ship")?.dataset.light).toBe("waiting");
    expect(node("a:release")?.getAttribute("aria-label")).toBe("release: in progress");
    // Edges draw between measured handles, which jsdom never lays out; the e2e sees them.

    await fireEvent.click(node("a:release")!);
    expect(onopen).toHaveBeenLastCalledWith("a:release");
    // The opened Artifact is marked in the graph.
    await rerender({ opened: "a:release" });
    expect(node("a:release")?.querySelector(".open")).not.toBeNull();
    expect(node("a:ship")?.querySelector(".open")).toBeNull();
    // A focused node takes Enter as a click; no other key opens anything.
    node("a:ship")!.focus();
    await fireEvent.keyDown(node("a:ship")!, { key: "ArrowLeft" });
    await fireEvent.keyDown(node("a:ship")!, { key: "o" });
    expect(onopen).toHaveBeenCalledTimes(1);
    await fireEvent.keyDown(node("a:ship")!, { key: "Enter" });
    expect(onopen).toHaveBeenLastCalledWith("a:ship");
    expect(document.activeElement).toBe(node("a:ship"));
    await fireEvent.click(document.querySelector<HTMLElement>(".svelte-flow__pane")!);
    expect(onclose).toHaveBeenCalledOnce();
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
    const props = { run: "run-a", revision: 1, onopen: vi.fn(), onclose: vi.fn() };
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

  it("asks before laying out more than the limit, once per Run", async () => {
    let size = LIMIT + 1;
    const many = (run: string): Graph => ({
      run,
      nodes: Array.from({ length: size }, (_, index) => ({ id: `a:${run}-${index}`, name: `${run}-${index}`, light: "green" })),
      edges: [],
    });
    mockIPC((cmd, args) => (cmd === "run_graph" ? many((args as { run: string }).run) : null));
    layout.mockImplementation(async (laid: Graph) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
    const props = { run: "run-a", revision: 1, onopen: vi.fn(), onclose: vi.fn() };
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
    const props = { run: "run-0", revision: 1, onopen: vi.fn(), onclose: vi.fn() };
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

  it("shows ERROR apart from RED, and a legend of all six lights", async () => {
    setup(() => ({
      run: "run-a",
      nodes: [
        { id: "a:release", name: "release", light: "red" },
        { id: "a:ship", name: "ship", light: "error" },
        { id: "a:docs", name: "docs", light: "blocked" },
      ],
      edges: [{ source: "a:ship", target: "a:docs", light: "error" }],
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
