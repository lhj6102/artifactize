// What the Run tree shows of the snapshot: folds, the selection and the blocker jumps. Rows
// and their meaning come from the backend; only this navigation state lives here.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import type { TreeNode, UpstreamMark } from "../ipc/client";

export type Row = { node: TreeNode; depth: number; path: string[] };

const same = (a: string[], b: string[]) => a.length === b.length && a.every((id, i) => id === b[i]);

/** The visible rows: the Run row, each Artifact, and the evals of open Artifacts. */
export function visible(tree: TreeNode[], opened: Set<string>): Row[] {
  return tree.flatMap((node) => [
    { node, depth: 0, path: [node.id] },
    ...(opened.has(node.id)
      ? node.children.map((child) => ({ node: child, depth: 1, path: [node.id, child.id] }))
      : []),
  ]);
}

/** The node a path points at. */
export function find(tree: TreeNode[], path: string[]): TreeNode | undefined {
  const top = tree.find((node) => node.id === path[0]);
  return path.length > 1 ? top?.children.find((node) => node.id === path[1]) : top;
}

export class Tree {
  readonly opened = new SvelteSet<string>();
  selected = $state<string[]>([]);
  /** A `b` jump: where it started and the Artifact row it reached. */
  blocker = $state<{ origin: string[]; at: string[] } | null>(null);
  #touched = new SvelteSet<string>();
  #run: string | null = null;

  /**
   * Fold Artifacts whose evals are all done and unfold the rest, except those the user
   * toggled or whose eval is selected. A new Run starts on its first in-progress or failed
   * eval, else its first row.
   */
  sync(run: string, tree: TreeNode[]): void {
    const first = run !== this.#run;
    this.#run = run;
    if (first) {
      this.opened.clear();
      this.#touched.clear();
      this.blocker = null;
    }
    for (const node of tree) {
      if (node.children.length === 0 || this.#touched.has(node.id)) continue;
      if (!node.done) this.opened.add(node.id);
      // An Artifact stays open while one of its evals is selected: the selection (and the
      // Detail that follows it) never jumps because a result arrived. It folds once the
      // selection leaves.
      else if (!(this.selected[0] === node.id && this.selected.length > 1)) this.opened.delete(node.id);
    }
    const artifacts = tree.filter((node) => node.kind === "artifact");
    if (first || !find(tree, this.selected)) {
      const start = artifacts
        .map((artifact) => {
          const eval_ = artifact.children.find((child) => child.attention);
          return eval_ ? [artifact.id, eval_.id] : null;
        })
        .find((path) => path !== null);
      this.selected = start ?? (artifacts[0] ? [artifacts[0].id] : tree[0] ? [tree[0].id] : []);
      if (start) this.opened.add(start[0]);
    }
  }

  select(path: string[]): void {
    this.selected = path;
  }

  /** Move the selection `delta` visible rows. */
  move(rows: Row[], delta: number): void {
    const index = rows.findIndex((row) => same(row.path, this.selected));
    const next = Math.min(Math.max(index + delta, 0), rows.length - 1);
    if (rows[next]) this.selected = rows[next].path;
  }

  #fold(id: string, open: boolean): void {
    if (open) this.opened.add(id);
    else this.opened.delete(id);
    this.#touched.add(id);
  }

  /** `h` / ←: fold an open Artifact, or step from an eval to its Artifact. */
  left(tree: TreeNode[]): void {
    const [top] = this.selected;
    if (this.selected.length > 1) this.selected = [top];
    else if (this.opened.has(top) && find(tree, [top])?.children.length) this.#fold(top, false);
  }

  /** `l`: unfold the selected Artifact. */
  right(tree: TreeNode[]): void {
    const [top] = this.selected;
    if (this.selected.length === 1 && find(tree, [top])?.children.length) this.#fold(top, true);
  }

  /** Space: fold or unfold the selected Artifact. */
  toggle(tree: TreeNode[], id = this.selected[0]): void {
    if (find(tree, [id])?.children.length) this.#fold(id, !this.opened.has(id));
  }

  /** The row whose upstream is marked: a `b` cycle's origin while the cursor stays where the
   * last jump left it, otherwise the selection. */
  origin(): string[] {
    return this.blocker && same(this.blocker.at, this.selected) ? this.blocker.origin : this.selected;
  }

  /** The Artifacts marked `↑`, by row id. */
  upstream(tree: TreeNode[]): Map<string, UpstreamMark> {
    const marks = find(tree, this.origin())?.upstream ?? [];
    return new SvelteMap(marks.map((mark) => [mark.node, mark]));
  }

  /** `b`: the next Artifact the origin row depends on, unmet first. */
  jumpBlocker(tree: TreeNode[]): void {
    const origin = this.origin();
    const targets = find(tree, origin)?.upstream ?? [];
    if (targets.length === 0) return;
    const index =
      this.blocker && same(this.blocker.at, this.selected)
        ? (targets.findIndex((mark) => mark.node === this.blocker?.at[0]) + 1) % targets.length
        : 0;
    const at = [targets[index].node];
    this.selected = at;
    this.blocker = { origin, at };
  }

  /** Backspace: back to the row the jumps started from, unfolding its Artifact. */
  jumpBack(): void {
    const blocker = this.blocker;
    this.blocker = null;
    if (!blocker || !same(blocker.at, this.selected)) return;
    if (blocker.origin.length > 1 && !this.opened.has(blocker.origin[0])) {
      this.#fold(blocker.origin[0], true);
    }
    this.selected = blocker.origin;
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
        if (entry.path.length > 1) this.opened.add(entry.path[0]);
        this.selected = entry.path;
        return true;
      }
    }
    return false;
  }
}
