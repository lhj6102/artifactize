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

export function graphAppearance(browser: WebdriverIO.Browser) {
  return browser.execute(() => {
    const painted = (element: Element | null) => {
      if (!element || element.getClientRects().length === 0) return false;
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility === "visible" && Number(style.opacity) > 0;
    };
    return {
      nodes: Object.fromEntries([...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => {
        const artifact = node.querySelector(".artifact");
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
      edges: Object.fromEntries([...document.querySelectorAll<SVGElement>(".svelte-flow__edge")].map((edge) => {
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
  const colours = Object.values(LABELS).map((label) => graph.legend[label]);
  if (!colours.every((entry) => entry?.painted && visibleColour(entry.colour)) ||
    new Set(colours.map((entry) => entry.colour)).size !== Object.keys(LABELS).length) return false;
  return Object.entries(nodes).every(([id, light]) => {
    const node = graph.nodes[id];
    const colour = graph.legend[LABELS[light]].colour;
    return node?.light === light && node.painted && node.border === colour && node.dot === colour;
  }) && Object.entries(edges).every(([id, light]) => {
    const edge = graph.edges[id];
    return edge?.light === light && edge.painted && edge.stroke === graph.legend[LABELS[light]].colour;
  });
}
