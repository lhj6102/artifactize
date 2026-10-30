// Read the graph's actual paint, not just the verdict attributes supplied to the renderer.
// Expected verdicts come from each fixture; the visible legend identifies their theme colours.
export type VerdictLight = "green" | "active" | "waiting" | "red" | "error" | "blocked";

const LABELS: Record<VerdictLight, string> = {
  green: "GREEN",
  active: "in progress",
  waiting: "waiting",
  red: "RED",
  error: "ERROR",
  blocked: "blocked by upstream",
};

/** What verify would do with a Current's Artifact, and its legend's words. */
export type CurrentLight = "reused" | "run" | "waiting" | "blocked" | "checking";

const CURRENT_LABELS: Record<CurrentLight, string> = {
  reused: "reused",
  run: "would run",
  waiting: "waiting",
  blocked: "blocked by upstream",
  checking: "checking",
};

export function graphAppearance(browser: WebdriverIO.Browser) {
  return browser.execute(() => {
    const painted = (element: Element | null) => {
      if (!element || element.getClientRects().length === 0) return false;
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility === "visible" && Number(style.opacity) > 0;
    };
    // The Run graph only: an open Detail draws an Artifact's neighbours with the same nodes.
    const ofRun = <E extends Element>(selector: string) =>
      [...document.querySelectorAll<E>(selector)].filter((element) => !element.closest('aside[aria-label="Detail"]'));
    return {
      nodes: Object.fromEntries(ofRun<HTMLElement>(".svelte-flow__node").map((node) => {
        // An Artifact's node, or a whole's box.
        const artifact = node.querySelector(".artifact, .whole");
        const dot = artifact?.querySelector(".dot") ?? null;
        const style = artifact ? getComputedStyle(artifact) : null;
        return [node.dataset.id ?? "", {
          light: node.dataset.light,
          at: node.style.transform,
          border: style?.borderTopColor ?? "",
          dot: dot ? getComputedStyle(dot).backgroundColor : "",
          painted: painted(node) && painted(artifact) && painted(dot) &&
            style?.borderTopStyle !== "none" && Number.parseFloat(style?.borderTopWidth ?? "0") > 0,
        }] as const;
      })),
      edges: Object.fromEntries(ofRun<SVGElement>(".svelte-flow__edge").map((edge) => {
        // The interaction path is transparent; only this path draws the relation.
        const path = edge.querySelector(".svelte-flow__edge-path");
        const style = path ? getComputedStyle(path) : null;
        return [edge.dataset.id ?? "", {
          light: edge.dataset.light,
          stroke: style?.stroke ?? "",
          painted: painted(edge) && painted(path) && !!path?.getAttribute("d") &&
            Number.parseFloat(style?.strokeWidth ?? "0") > 0 && Number(style?.strokeOpacity ?? "0") > 0,
        }] as const;
      })),
      legend: Object.fromEntries([...document.querySelectorAll('[aria-label="Legend"] li')].map((item) => {
        const swatch = item.querySelector(".swatch");
        return [item.textContent?.trim() ?? "", {
          colour: swatch ? getComputedStyle(swatch).backgroundColor : "",
          painted: painted(item) && painted(swatch),
        }] as const;
      })),
    };
  });
}

export type GraphAppearance = Awaited<ReturnType<typeof graphAppearance>>;

export function visibleColour(colour: string): boolean {
  return colour !== "" && colour !== "none" && colour !== "transparent" &&
    !/^rgba\([^)]*,\s*0\s*\)$/.test(colour);
}

export function graphHasColours(
  graph: GraphAppearance,
  nodes: Record<string, VerdictLight>,
  edges: Record<string, VerdictLight>,
): boolean {
  return hasColours(graph, LABELS, nodes, edges);
}

/** A Current's graph in its own legend's colours: each entry distinct, and each given node
 * and relation painted as the plan the fixture expects of it. */
export function currentHasColours(
  graph: GraphAppearance,
  nodes: Record<string, CurrentLight>,
  edges: Record<string, CurrentLight>,
): boolean {
  return hasColours(graph, CURRENT_LABELS, nodes, edges);
}

function hasColours<L extends string>(
  graph: GraphAppearance,
  labels: Record<L, string>,
  nodes: Record<string, L>,
  edges: Record<string, L>,
): boolean {
  const colours = Object.values<string>(labels).map((label) => graph.legend[label]);
  if (!colours.every((entry) => entry?.painted && visibleColour(entry.colour)) ||
    new Set(colours.map((entry) => entry.colour)).size !== colours.length) return false;
  return Object.entries<L>(nodes).every(([id, light]) => {
    const node = graph.nodes[id];
    const colour = graph.legend[labels[light]].colour;
    return node?.light === light && node.painted && node.border === colour && node.dot === colour;
  }) && Object.entries<L>(edges).every(([id, light]) => {
    const edge = graph.edges[id];
    return edge?.light === light && edge.painted && edge.stroke === graph.legend[labels[light]].colour;
  });
}
