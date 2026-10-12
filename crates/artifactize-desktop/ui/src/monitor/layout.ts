// Node positions for the Run graph: ELK's layered algorithm, left to right, upstream first.
// ELK runs in a Web Worker started on first use, so a large layout never blocks the window and
// the tree view never loads it.
import type { RunGraph } from "../ipc/client";
import type { Point } from "./graph";

/** Every Artifact node's box; names longer than fit are cut with an ellipsis. */
export const NODE = { width: 168, height: 44 };

type Elk = { layout: (graph: object) => Promise<{ children?: { id: string; x?: number; y?: number }[] }> };
let elk: Promise<Elk> | null = null;

/** Top-left positions of `graph`'s nodes. */
export async function layout(graph: RunGraph): Promise<Map<string, Point>> {
  elk ??= Promise.all([import("elkjs/lib/elk-api.js"), import("elkjs/lib/elk-worker.min.js?url")]).then(
    ([{ default: ELK }, { default: workerUrl }]) => new ELK({ workerUrl }) as Elk,
  );
  const laid = await (
    await elk
  ).layout({
    id: "run",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.layered.spacing.nodeNodeBetweenLayers": "72",
      "elk.spacing.nodeNode": "20",
      "elk.spacing.componentComponent": "40",
      // Keep the tree's order (upstream first) within a layer where crossings allow.
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
    },
    children: graph.nodes.map((node) => ({ id: node.id, ...NODE })),
    edges: graph.edges.map((edge, index) => ({ id: `e${index}`, sources: [edge.source], targets: [edge.target] })),
  });
  return new Map((laid.children ?? []).map((child) => [child.id, { x: child.x ?? 0, y: child.y ?? 0 }]));
}
