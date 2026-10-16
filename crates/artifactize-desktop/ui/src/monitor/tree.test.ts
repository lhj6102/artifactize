// The Monitor's selection over the backend's Run rows: the starting node, blocker jumps and the
// next attention item.
import { describe, expect, it } from "vitest";
import type { TreeNode } from "../ipc/client";
import { Tree } from "./tree.svelte";

function node(id: string, extra: Partial<TreeNode> = {}): TreeNode {
  return {
    id,
    kind: id.startsWith("run:") ? "run" : id.startsWith("a:") ? "artifact" : "eval",
    glyph: "·",
    tone: "muted",
    weight: "normal",
    name: id.slice(id.indexOf(":") + 1),
    marks: "",
    text: [],
    right: "",
    changed: false,
    done: false,
    attention: false,
    urgent: false,
    upstream: [],
    children: [],
    ...extra,
  };
}

const tree = (): TreeNode[] => [
  node("run:run-a"),
  node("a:docs", { done: true, children: [node("e:docs/lint")] }),
  node("a:release", { children: [node("e:release/signoff", { attention: true, urgent: true })] }),
  node("a:ship", {
    upstream: [{ node: "a:release", complete: false }],
    children: [node("e:ship/check", { upstream: [{ node: "a:release", complete: false }] })],
  }),
];

describe("Tree", () => {
  it("starts a new Run on its first in-progress or failed eval and keeps a selection it still has", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
    state.select(["a:docs", "e:docs/lint"]);
    state.sync("run-a", tree());
    expect(state.selected).toEqual(["a:docs", "e:docs/lint"]);
    // A selection the snapshot lost starts over, and so does another Run.
    state.sync("run-a", tree().filter((node) => node.id !== "a:docs"));
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
    state.select(["a:ship"]);
    state.sync("run-b", tree());
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
  });

  it("starts on the first Artifact when nothing is in progress", () => {
    const state = new Tree();
    const calm = tree().map((top) => ({ ...top, children: top.children.map((child) => ({ ...child, attention: false })) }));
    state.sync("run-a", calm);
    expect(state.selected).toEqual(["a:docs"]);
  });

  it("b cycles to the upstream Artifacts and Backspace returns to the origin", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    state.select(["a:ship", "e:ship/check"]);
    expect(state.jumpBlocker(tree())).toBe(true);
    expect(state.selected).toEqual(["a:release"]);
    // The origin stays while the selection sits on the jump target.
    expect(state.origin()).toEqual(["a:ship", "e:ship/check"]);
    expect(state.jumpBack()).toBe(true);
    expect(state.selected).toEqual(["a:ship", "e:ship/check"]);
    // Nothing upstream, nothing to go back to: neither moves.
    state.select(["a:docs"]);
    expect(state.jumpBlocker(tree())).toBe(false);
    expect(state.jumpBack()).toBe(false);
    expect(state.selected).toEqual(["a:docs"]);
  });

  it("! visits ERROR, RED and waiting Human evals, wrapping around", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    state.select(["a:ship"]);
    const moved = state.nextAttention(tree());
    expect(moved).toBe(true);
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
    const calm = tree().map((top) => ({ ...top, children: top.children.map((child) => ({ ...child, urgent: false })) }));
    const stayed = state.nextAttention(calm);
    expect(stayed).toBe(false);
  });
});
