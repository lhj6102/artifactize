import { renderHosted as render } from "../test-host";
// The Run graph against a mocked command boundary and layout: nodes take their lights, a click
// (or Enter on a focused node) on an Artifact asks for its Detail and one on the background
// closes it, and a new snapshot recolours in place.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import type { RunGraph as Graph, Shape } from "../ipc/client";
import RunGraph from "./RunGraph.svelte";

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

function graph(release: Shape["nodes"][number]["light"], ship = "waiting" as const): Shape {
  return {
    nodes: [
      { id: "a:release", name: "release", light: release },
      { id: "a:ship", name: "ship", light: ship },
    ],
    edges: [{ source: "a:release", target: "a:ship", light: release }],
  };
}

/** The backend's answer for Run `run` now serving `shape`: its lights alone when the caller
 * holds that shape, else the shape. A shape is named by its Artifacts and edges. */
function answer(run: string, shape: Shape, known: number | null): Graph {
  const named = [shape.nodes.map((node) => node.id), shape.edges.map((edge) => `${edge.source}>${edge.target}`)].join("|");
  const key = names.get(named) ?? names.set(named, names.size + 1).get(named)!;
  return {
    run,
    shape: key,
    update: known === key
      ? { kind: "lights", nodes: shape.nodes.map((node) => node.light), edges: shape.edges.map((edge) => edge.light) }
      : { kind: "shape", ...shape },
  };
}
const names = new Map<string, number>();

/** Every `run_graph` call's arguments, in order. */
let asked: { run: string; known: number | null }[] = [];

function serve(served: (run: string) => Shape) {
  asked = [];
  mockIPC((cmd, args) => {
    if (cmd !== "run_graph") return null;
    const { run, known } = args as { run: string; known: number | null };
    asked.push({ run, known });
    return answer(run, served(run), known);
  });
}

function setup(served: () => Shape) {
  serve(served);
  layout.mockImplementation((laid: Shape) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
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

  it("asks only for the lights of the shape it holds, recolours them in place, and lays out only a new shape", async () => {
    let served = graph("active");
    const { rerender } = setup(() => served);
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("active"));
    expect(layout).toHaveBeenCalledOnce();
    const shown = asked[0];
    expect(shown.known).toBeNull();
    // Showing what it read does not read again: only a new snapshot does.
    await tick();
    await tick();
    expect(asked).toHaveLength(1);

    served = graph("green");
    await rerender({ revision: 2 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    // The second read named the shape the first one gave, and only relit it.
    expect(asked[1].known).not.toBeNull();
    expect(layout).toHaveBeenCalledOnce();

    served = { ...graph("green"), nodes: [...graph("green").nodes, { id: "a:docs", name: "docs", light: "waiting" }] };
    await rerender({ revision: 3 });
    await waitFor(() => expect(node("a:docs")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
  });

  it("never lets an older read of another Run replace the Run shown now", async () => {
    // Run A answers at once; Run B's answer waits until the test releases it.
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    let asking!: () => void;
    const askedB = new Promise<void>((done) => (asking = done));
    let lightA = graph("active");
    mockIPC(async (cmd, args) => {
      if (cmd !== "run_graph") return null;
      const { run, known } = args as { run: string; known: number | null };
      if (run === "run-b") {
        asking();
        await held;
        return answer("run-b", { nodes: [{ id: "a:other", name: "other", light: "red" }], edges: [] }, known);
      }
      return answer(run, lightA, known);
    });
    layout.mockImplementation((laid: Shape) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
    const { rerender } = render(RunGraph, { run: "run-a", revision: 1, onopen: vi.fn(), onclose: vi.fn() });
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    await rerender({ run: "run-b", revision: 2 });
    await askedB;
    lightA = graph("green");
    await rerender({ run: "run-a", revision: 3 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    release();
    // Let the held answer arrive, then flush any view update before asserting that it did not
    // replace Run A.
    await held;
    await tick();
    await tick();
    expect(node("a:other")).toBeNull();
    expect(node("a:release")?.dataset.light).toBe("green");
  });

  it("explains a Run without Artifacts", async () => {
    setup(() => ({ nodes: [], edges: [] }));
    await screen.findByText("This Run has no Artifacts.");
  });

  it("shows ERROR apart from RED, and a legend of all six lights", async () => {
    setup(() => ({
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
