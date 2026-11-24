// What a project declares under an `artifactize` Artifact, as the library's declared graph reads
// (its scoped wire shape): the Artifacts, their evals and the references between them.

/** A declared Artifact: its file or folder kind, and its type (`null` for a Custom one). */
export type DeclaredArtifact = { kind: string; type?: string | null };
export type DeclaredEval = {
  target: string;
  title: string;
  criteria: { kind: "agent" | "human" | "runtime"; command?: string };
  payload: { instruction: string } | null;
  reads?: string[];
};
export type Declared = {
  artifacts: Record<string, DeclaredArtifact>;
  evals: Record<string, DeclaredEval>;
  relations: { source: string; target: string; kind: string }[];
};

/** Read the library's declared graph `json`. */
export function declared(json: string): Declared {
  const read = JSON.parse(json) as Partial<Declared>;
  return { artifacts: read.artifacts ?? {}, evals: read.evals ?? {}, relations: read.relations ?? [] };
}

/** Artifact `name`'s evals, by their IDs, in order. */
export function evalsOf(graph: Declared, name: string): [string, DeclaredEval][] {
  return Object.entries(graph.evals).filter(([, each]) => each.target === name);
}

/** The edges between declared Artifacts: one per reading pair, whatever references it, never a
 * part's own (a part is drawn by the reference to it, not as an eval). */
export function edges(graph: Declared): { source: string; target: string }[] {
  const seen = new Set<string>();
  const out: { source: string; target: string }[] = [];
  for (const { source, target } of graph.relations) {
    if (!(source in graph.artifacts) || !(target in graph.artifacts) || source === target) continue;
    const key = `${source}\u0000${target}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ source, target });
  }
  return out;
}

/** What an eval's criteria say, whole: its instruction, else the command it runs. */
export function criteria(each: DeclaredEval): string {
  return each.payload?.instruction ?? each.criteria.command ?? "";
}
