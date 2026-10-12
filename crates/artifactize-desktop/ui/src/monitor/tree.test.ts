// The Run tree's navigation state over backend rows: folds, the starting row, blocker jumps
// and the next attention item.
import { describe, expect, it } from "vitest";
import type { TreeNode } from "../ipc/client";
import { Tree, visible } from "./tree.svelte";

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
  it("folds done Artifacts, opens the rest and starts on the first in-progress or failed eval", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    expect(state.opened.has("a:docs")).toBe(false);
    expect(state.opened.has("a:release")).toBe(true);
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
    expect(visible(tree(), state.opened).map((row) => row.node.id)).toEqual([
      "run:run-a",
      "a:docs",
      "a:release",
      "e:release/signoff",
      "a:ship",
      "e:ship/check",
    ]);
  });

  it("keeps a fold the user chose across refreshes until the Run changes", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    state.select(["a:docs"]);
    state.toggle(tree());
    state.sync("run-a", tree());
    expect(state.opened.has("a:docs")).toBe(true);
    state.sync("run-b", tree());
    expect(state.opened.has("a:docs")).toBe(false);
  });

  it("b cycles to the upstream Artifacts and Backspace returns to the origin row", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    state.select(["a:ship", "e:ship/check"]);
    expect([...state.upstream(tree()).keys()]).toEqual(["a:release"]);
    state.jumpBlocker(tree());
    expect(state.selected).toEqual(["a:release"]);
    // The origin's marks stay while the cursor sits on the jump target.
    expect([...state.upstream(tree()).keys()]).toEqual(["a:release"]);
    state.jumpBack();
    expect(state.selected).toEqual(["a:ship", "e:ship/check"]);
  });

  it("! visits ERROR, RED and waiting Human evals, wrapping around", () => {
    const state = new Tree();
    state.sync("run-a", tree());
    state.select(["a:ship"]);
    expect(state.nextAttention(tree())).toBe(true);
    expect(state.selected).toEqual(["a:release", "e:release/signoff"]);
    const calm = tree().map((top) => ({ ...top, children: top.children.map((child) => ({ ...child, urgent: false })) }));
    expect(state.nextAttention(calm)).toBe(false);
  });
});
