// A Run graph of a whole catalog's shape, for tests and benchmarks at the size the desktop app
// must keep quick: about 21,500 Artifacts and 49,000 edges, one Artifact used by 16,534 of them.
// Each part is a skill: nine tooltip Artifacts and thirty reviewed chains, each chain using its
// parent chain, one tooltip and the shared criteria file, as a real catalog Run's are.
import type { Shape } from "../ipc/client";

/** The reviews of the catalog, each using the shared criteria. */
export const REVIEWS = 16534;
/** Chains per skill and tooltips per skill. */
const CHAINS = 30;
const TOOLTIPS = 9;
/** The Artifact every review uses. */
export const SHARED = "a:review-criteria";

/** The catalog graph, every Artifact and edge lit `light`, in a tree's order: the shared input,
 * then each skill's tooltips and chains. */
export function catalog(light: Shape["nodes"][number]["light"] = "waiting"): Shape {
  const nodes: Shape["nodes"] = [{ id: SHARED, name: "review-criteria", space: null, label: "review-criteria", light, note: null }];
  const edges: Shape["edges"] = [];
  const edge = (source: string, target: string) => edges.push({ source, target, light });
  for (let skill = 0, reviews = 0; reviews < REVIEWS; skill += 1) {
    const tips = Array.from({ length: TOOLTIPS }, (_, at) => `a:t${skill}-${at}`);
    for (const id of tips) nodes.push({ id, name: id.slice(2), space: null, label: id.slice(2), light, note: null });
    const chains = Math.min(CHAINS, REVIEWS - reviews);
    for (let chain = 0; chain < chains; chain += 1, reviews += 1) {
      const id = `a:c${skill}-${chain}`;
      nodes.push({ id, name: id.slice(2), space: null, label: id.slice(2), light, note: null });
      if (chain > 0) edge(`a:c${skill}-${Math.floor((chain - 1) / 3)}`, id);
      edge(tips[chain % TOOLTIPS], id);
      edge(SHARED, id);
    }
  }
  return { nodes, edges };
}
