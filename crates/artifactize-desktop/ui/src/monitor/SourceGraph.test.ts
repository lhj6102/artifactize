import { renderHosted as render } from "../test-host";
// A Run's or a Current's graph against a mocked command boundary and layout: nodes take their lights, a click
// (or Enter on a focused node) on an Artifact asks for its Detail and one on the background
// closes it, a new snapshot recolours in place, and a new shape replaces the graph once laid out.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Graph, Shape, Source } from "../ipc/client";
import SourceGraph from "./SourceGraph.svelte";

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

const source = (run: string): Source => ({ kind: "run", run });

/** The Run a `graph` call asks for and the shape it holds. */
function runOf(args: unknown): { run: string; known: number | null } {
  const { source, known } = args as { source: Source; known: number | null };
  return { run: source.kind === "run" ? source.run : "", known };
}

function graph(release: Shape["nodes"][number]["light"], ship = "waiting" as const): Shape {
  return {
    nodes: [
      { id: "a:release", name: "release", light: release, note: null },
      { id: "a:ship", name: "ship", light: ship, note: null },
    ],
    edges: [{ source: "a:release", target: "a:ship", light: release }],
  };
}

/** The backend's answer for Run `run` now serving `shape`: its lights alone when the caller
 * holds that shape, else the shape. A shape is named by its Artifacts and edges. */
function answer(run: string, shape: Shape, known: number | null): Graph {
  const source: Source = { kind: "run", run };
  const named = [shape.nodes.map((node) => node.id), shape.edges.map((edge) => `${edge.source}>${edge.target}`)].join("|");
  const key = names.get(named) ?? names.set(named, names.size + 1).get(named)!;
  return {
    source,
    shape: key,
    update: known === key
      ? { kind: "lights", nodes: shape.nodes.map((node) => node.light), edges: shape.edges.map((edge) => edge.light) }
      : { kind: "shape", ...shape },
  };
}
const names = new Map<string, number>();

/** Every `graph` call's Run and the shape it held, in order. */
let asked: { run: string; known: number | null }[] = [];

function serve(served: (run: string) => Shape) {
  asked = [];
  mockIPC((cmd, args) => {
    if (cmd !== "graph") return null;
    const { run, known } = runOf(args);
    asked.push({ run, known });
    return answer(run, served(run), known);
  });
}

function setup(served: () => Shape) {
  serve(served);
  layout.mockImplementation(async (laid: Shape) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
  const onopen = vi.fn();
  const onclose = vi.fn();
  const view = render(SourceGraph, { source: source("run-a"), revision: 1, onopen, onclose });
  return { ...view, onopen, onclose };
}

const node = (id: string) => document.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);

describe("SourceGraph", () => {
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

    served = graph("green");
    await rerender({ revision: 2 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    // Only the new snapshot read again: showing a read does not read once more. The second read
    // named the shape the first one gave, and only relit it.
    expect(asked).toHaveLength(2);
    expect(asked[1].known).not.toBeNull();
    expect(layout).toHaveBeenCalledOnce();

    served = { ...graph("green"), nodes: [...graph("green").nodes, { id: "a:docs", name: "docs", light: "waiting", note: null }] };
    await rerender({ revision: 3 });
    await waitFor(() => expect(node("a:docs")).not.toBeNull());
    expect(layout).toHaveBeenCalledTimes(2);
  });

  it("keeps the graph shown while a new shape lays out, and lays out each shape once whatever snapshots arrive", async () => {
    const docs = { id: "a:docs", name: "docs", light: "waiting" as const, note: null };
    const grown = (release: Shape["nodes"][number]["light"]): Shape => ({ ...graph(release), nodes: [...graph(release).nodes, docs] });
    let served = graph("active");
    const { rerender } = setup(() => served);
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("active"));
    // The grown shape's layout finishes only when the test says so.
    let finish!: () => void;
    const finished = new Promise<void>((done) => (finish = done));
    layout.mockImplementation(async (laid: Shape) => {
      await finished;
      return new Map(laid.nodes.map((each, index) => [each.id, { x: index * 240, y: 0 }]));
    });

    served = grown("green");
    await rerender({ revision: 2 });
    await waitFor(() => expect(layout).toHaveBeenCalledTimes(2));
    // Another snapshot of the grown shape arrives while it lays out.
    served = grown("red");
    await rerender({ revision: 3 });
    await waitFor(() => expect(asked).toHaveLength(3));
    // Nothing shown can change before the layout finishes: the graph before stays, lit as it was.
    expect(screen.queryByText("Laying out…")).toBeNull();
    expect(node("a:release")?.dataset.light).toBe("active");
    expect(node("a:docs")).toBeNull();

    finish();
    await waitFor(() => expect(node("a:docs")).not.toBeNull());
    expect(node("a:release")?.dataset.light).toBe("red");
    expect(layout).toHaveBeenCalledTimes(2);
    // Later lights of the grown shape relight it without laying it out again.
    served = grown("green");
    await rerender({ revision: 4 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
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
      if (cmd !== "graph") return null;
      const { run, known } = runOf(args);
      if (run === "run-b") {
        asking();
        await held;
        return answer("run-b", { nodes: [{ id: "a:other", name: "other", light: "red", note: null }], edges: [] }, known);
      }
      return answer(run, lightA, known);
    });
    layout.mockImplementation(async (laid: Shape) => new Map(laid.nodes.map((node, index) => [node.id, { x: index * 240, y: 0 }])));
    const { rerender } = render(SourceGraph, { source: source("run-a"), revision: 1, onopen: vi.fn(), onclose: vi.fn() });
    await waitFor(() => expect(node("a:release")).not.toBeNull());
    await rerender({ source: source("run-b"), revision: 2 });
    await askedB;
    lightA = graph("green");
    await rerender({ source: source("run-a"), revision: 3 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("green"));
    release();
    // Run B's held answer arrives before the answer to a newer read of Run A, which asks only
    // after the release: once that newer read shows, B's answer has been handled.
    await held;
    lightA = graph("red");
    await rerender({ source: source("run-a"), revision: 4 });
    await waitFor(() => expect(node("a:release")?.dataset.light).toBe("red"));
    expect(node("a:other")).toBeNull();
  });

  it("explains a Run without Artifacts", async () => {
    setup(() => ({ nodes: [], edges: [] }));
    await screen.findByText("This Run has no Artifacts.");
  });

  it("shows ERROR apart from RED, and a legend of all six lights", async () => {
    setup(() => ({
      nodes: [
        { id: "a:release", name: "release", light: "red", note: null },
        { id: "a:ship", name: "ship", light: "error", note: null },
        { id: "a:docs", name: "docs", light: "blocked", note: null },
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
  it("marks a Current's Artifacts by what verify would do, with a legend of its own", async () => {
    const current: Source = { kind: "current", branch: { kind: "branch", common: "/work/app/.git", branch: "main" } };
    mockIPC((cmd, args) =>
      cmd === "graph"
        ? {
            source: (args as { source: Source }).source,
            shape: 1,
            update: {
              kind: "shape",
              nodes: [
                { id: "a:app", name: "app", light: "reused", note: "reused" },
                { id: "a:graph", name: "graph", light: "run", note: "would run (2 evals)" },
              ],
              edges: [{ source: "a:app", target: "a:graph", light: "reused" }],
            },
          }
        : null,
    );
    layout.mockImplementation(async (laid: Shape) => new Map(laid.nodes.map((item, index) => [item.id, { x: index * 240, y: 0 }])));
    render(SourceGraph, { source: current, revision: 1, onopen: vi.fn(), onclose: vi.fn() });
    await waitFor(() => expect(node("a:graph")).not.toBeNull());
    expect(node("a:graph")?.getAttribute("aria-label")).toBe("graph: would run (2 evals)");
    // Drawn by its colour alone, as a Run's Artifacts are: the node shows only its name.
    expect(node("a:graph")?.textContent?.trim()).toBe("graph");
    expect(node("a:app")?.dataset.light).toBe("reused");
    const legend = [...screen.getByRole("list", { name: "Legend" }).querySelectorAll("li")].map((item) => item.textContent?.trim());
    expect(legend).toEqual(["reused", "would run", "waiting", "blocked by upstream", "checking"]);
  });
});
