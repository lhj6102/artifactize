// Read the graph's actual paint, not just the verdict attributes supplied to the renderer.
// Expected verdicts come from each fixture; the app's own status colours identify them: a node
// shows its status twice in one colour, its mark and its strip, its outline neutral; an edge is
// neutral unless it leaves a RED, ERROR or blocked Artifact, then drawn in that colour.
export type VerdictLight = "green" | "active" | "waiting" | "red" | "error" | "blocked";

/** Each light's status colour, as the app's tokens name it. */
const TOKENS: Record<VerdictLight, string> = {
  green: "--green",
  active: "--running",
  waiting: "--queued",
  red: "--red",
  error: "--error",
  blocked: "--blocked",
};

/** What verify would do with a Current's Artifact, and its status colour. */
export type CurrentLight = "reused" | "run" | "waiting" | "blocked" | "checking";

const CURRENT_TOKENS: Record<CurrentLight, string> = {
  reused: "--reused",
  run: "--running",
  waiting: "--queued",
  blocked: "--blocked",
  checking: "--checking",
};

/** The lights whose outgoing edges take their colour. */
const BLOCKING = new Set(["red", "error", "blocked"]);

/** The neutral line of edges. */
const NEUTRAL = ["--edge"];

export function graphAppearance(browser: WebdriverIO.Browser) {
  const tokens = [...new Set([...Object.values(TOKENS), ...Object.values(CURRENT_TOKENS), ...NEUTRAL])];
  return browser.execute((names) => {
    const painted = (element: Element | null) => {
      if (!element || element.getClientRects().length === 0) return false;
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility === "visible" && Number(style.opacity) > 0;
    };
    /** The colour of an inset box shadow (a strip), or "". */
    const strip = (element: Element | null) => {
      const shadow = element ? getComputedStyle(element).boxShadow : "";
      const inset = shadow.split(/,(?![^(]*\))/).find((part) => part.includes("inset"));
      return inset?.match(/rgba?\([^)]*\)|color\([^)]*\)|oklch\([^)]*\)/)?.[0] ?? "";
    };
    // Each token resolved as the engine paints it.
    const probe = document.body.appendChild(document.createElement("span"));
    const palette = Object.fromEntries(names.map((name) => {
      probe.style.color = `var(${name})`;
      return [name, getComputedStyle(probe).color] as const;
    }));
    probe.remove();
    // The Run graph only: an open Detail draws an Artifact's neighbours with the same nodes.
    const ofRun = <E extends Element>(selector: string) =>
      [...document.querySelectorAll<E>(selector)].filter((element) => !element.closest('aside[aria-label="Detail"]'));
    return {
      palette,
      nodes: Object.fromEntries(ofRun<HTMLElement>(".svelte-flow__node").map((node) => {
        // An Artifact's node, or a whole's box (its strip under its title unless minimized).
        const artifact = node.querySelector(".artifact, .whole");
        const mark = artifact?.querySelector(".light") ?? null;
        const style = artifact ? getComputedStyle(artifact) : null;
        const striped = artifact?.matches(".whole:not(.minimized)") ? artifact.querySelector(".title") : artifact;
        return [node.dataset.id ?? "", {
          light: node.dataset.light,
          at: node.style.transform,
          border: style?.borderTopColor ?? "",
          mark: mark ? getComputedStyle(mark).color : "",
          strip: strip(striped ?? null),
          painted: painted(node) && painted(artifact) && painted(mark?.querySelector("svg") ?? null) &&
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
    };
  }, tokens);
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
  return hasColours(graph, TOKENS, nodes, edges);
}

/** A Current's graph in its status colours: each given node and relation painted as the plan
 * the fixture expects of it. */
export function currentHasColours(
  graph: GraphAppearance,
  nodes: Record<string, CurrentLight>,
  edges: Record<string, CurrentLight>,
): boolean {
  return hasColours(graph, CURRENT_TOKENS, nodes, edges);
}

/** Every status colour of `tokens` is visible and its own; each node's mark and strip are its
 * light's colour within a neutral outline; each edge is neutral, or its source's colour when
 * that source holds others up. */
function hasColours<L extends string>(
  graph: GraphAppearance,
  tokens: Record<L, string>,
  nodes: Record<string, L>,
  edges: Record<string, L>,
): boolean {
  const colours = Object.values<string>(tokens).map((token) => graph.palette[token]);
  if (!colours.every(visibleColour) || new Set(colours).size !== colours.length) return false;
  return Object.entries<L>(nodes).every(([id, light]) => {
    const node = graph.nodes[id];
    const colour = graph.palette[tokens[light]];
    // The outline stays neutral: it never takes the status colour.
    return node?.light === light && node.painted && node.mark === colour && node.strip === colour && node.border !== colour;
  }) && Object.entries<L>(edges).every(([id, light]) => {
    const edge = graph.edges[id];
    const colour = BLOCKING.has(light) ? graph.palette[tokens[light]] : graph.palette["--edge"];
    return edge?.light === light && edge.painted && edge.stroke === colour;
  });
}
