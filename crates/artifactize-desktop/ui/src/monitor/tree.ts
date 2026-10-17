// The Run's tree of Artifacts and evals comes from the backend; this finds a row in it.
import type { TreeNode } from "../ipc/client";

/** The node a path points at: an Artifact (or the Run) and, below it, one of its evals. */
export function find(tree: TreeNode[], path: string[]): TreeNode | undefined {
  const top = tree.find((node) => node.id === path[0]);
  return path.length > 1 ? top?.children.find((node) => node.id === path[1]) : top;
}
