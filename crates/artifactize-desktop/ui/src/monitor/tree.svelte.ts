// What the Monitor selects in the Run's tree of Artifacts and evals: the selection and the
// blocker jumps. Rows and their meaning come from the backend; only this navigation state
// lives here. The graph shows the selection: an Artifact by itself, an eval in its Artifact's
// popup.
import type { TreeNode } from "../ipc/client";

const same = (a: string[], b: string[]) => a.length === b.length && a.every((id, i) => id === b[i]);

/** The node a path points at. */
export function find(tree: TreeNode[], path: string[]): TreeNode | undefined {
  const top = tree.find((node) => node.id === path[0]);
  return path.length > 1 ? top?.children.find((node) => node.id === path[1]) : top;
}

export class Tree {
  selected = $state<string[]>([]);
  /** A `b` jump: where it started and the Artifact it reached. */
  blocker = $state<{ origin: string[]; at: string[] } | null>(null);
  #run: string | null = null;

  /** A new Run starts on its first in-progress or failed eval, else its first Artifact; a
   * selection the Run no longer has starts over the same way. */
  sync(run: string, tree: TreeNode[]): void {
    const first = run !== this.#run;
    this.#run = run;
    if (first) this.blocker = null;
    if (!first && find(tree, this.selected)) return;
    const artifacts = tree.filter((node) => node.kind === "artifact");
    const start = artifacts
      .map((artifact) => {
        const eval_ = artifact.children.find((child) => child.attention);
        return eval_ ? [artifact.id, eval_.id] : null;
      })
      .find((path) => path !== null);
    this.selected = start ?? (artifacts[0] ? [artifacts[0].id] : tree[0] ? [tree[0].id] : []);
  }

  select(path: string[]): void {
    this.selected = path;
  }

  /** The node whose upstream `b` follows: a `b` cycle's origin while the selection stays where
   * the last jump left it, otherwise the selection. */
  origin(): string[] {
    return this.blocker && same(this.blocker.at, this.selected) ? this.blocker.origin : this.selected;
  }

  /** `b`: the next Artifact the origin depends on, unmet first. Says whether it moved. */
  jumpBlocker(tree: TreeNode[]): boolean {
    const origin = this.origin();
    const targets = find(tree, origin)?.upstream ?? [];
    if (targets.length === 0) return false;
    const index =
      this.blocker && same(this.blocker.at, this.selected)
        ? (targets.findIndex((mark) => mark.node === this.blocker?.at[0]) + 1) % targets.length
        : 0;
    const at = [targets[index].node];
    this.selected = at;
    this.blocker = { origin, at };
    return true;
  }

  /** Backspace: back to the node the jumps started from. Says whether it moved. */
  jumpBack(): boolean {
    const blocker = this.blocker;
    this.blocker = null;
    if (!blocker || !same(blocker.at, this.selected)) return false;
    this.selected = blocker.origin;
    return true;
  }

  /** `!`: the next ERROR, RED or waiting Human eval after the selection, wrapping around. */
  nextAttention(tree: TreeNode[]): boolean {
    const order = tree
      .filter((node) => node.kind !== "run")
      .flatMap((node) => [
        { path: [node.id], node },
        ...node.children.map((child) => ({ path: [node.id, child.id], node: child })),
      ]);
    const start = order.findIndex((entry) => same(entry.path, this.selected)) + 1;
    for (let offset = 0; offset < order.length; offset += 1) {
      const entry = order[(start + offset) % order.length];
      if (entry.node.urgent) {
        this.selected = entry.path;
        return true;
      }
    }
    return false;
  }
}
