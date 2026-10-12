// The Run graph against a mocked command boundary and layout: nodes take their lights, a click
// zooms into an Artifact and lists its evals from the tree, selecting one moves the tree's
// selection and Enter opens Detail, Esc zooms out, and a new snapshot recolours in place.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { RunGraph as Graph, TreeNode } from "../ipc/client";
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
});
