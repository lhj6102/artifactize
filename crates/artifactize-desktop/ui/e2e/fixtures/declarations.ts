// Definitions a fixture writes into its repository, as `.artifactize/{name}.artf` files the
// CLI reads, and the stand-in program their runtime evals and Human tools run.
import { nodeProgram, portablePath } from "../platform.ts";
import { makeDirectory, nativePath, parentOf, suitePath, writeText, type NativePath } from "../portable.ts";

type Node = { name: string; target: string; source: string; declared: string[] };
const projects = new Map<NativePath, Map<string, Node>>();

/** Declare the Artifact `source` names (`name = "..."`) at the folder of `targetFile` in `repo`:
 * its definition gets that folder as its `path`, and every Artifact whose folder holds it lists
 * it among its parts, as a project laid out in folders reads. */
export function writeDeclaration(repo: NativePath, targetFile: NativePath, source: string): void {
  const named = /^name\s*=\s*("(?:\\.|[^"\\])*")\s*$/m.exec(source);
  if (!named) throw new Error("a fixture declaration needs its name");
  const name = JSON.parse(named[1]) as string;
  const folder = portablePath(parentOf(targetFile)).slice(portablePath(repo).length).replace(/^\//, "");
  const target = folder ? `${folder}/` : "./";
  const declaredText = /^parts\s*=\s*(\[[^\n]*\])/m.exec(source)?.[1];
  const node = { name, target, source: source.replace(named[0], `path = ${JSON.stringify(target)}`), declared: declaredText ? JSON.parse(declaredText) as string[] : [] };
  const nodes = projects.get(repo) ?? new Map<string, Node>();
  projects.set(repo, nodes);
  nodes.set(name, node);
  const directory = makeDirectory(nativePath(repo, ".artifactize"));
  makeDirectory(parentOf(targetFile));
  // Git keeps no empty folder: a worktree of the repository still has every target.
  writeText(nativePath(parentOf(targetFile), ".gitkeep"), "");
  const inside = (whole: Node, part: Node) => whole.target === "./" || part.target.startsWith(whole.target);
  for (const whole of nodes.values()) {
    if (whole !== node && !inside(whole, node)) continue;
    const children = [...nodes.values()].filter((part) => part !== whole && inside(whole, part) && ![...nodes.values()].some((between) =>
      between !== whole && between !== part && between.target.length > whole.target.length && inside(whole, between) && part.target.startsWith(between.target)));
    const parts = [...new Set([...whole.declared, ...children.map((part) => part.name)])].sort();
    const text = !parts.length ? whole.source : /^parts\s*=/m.test(whole.source)
      ? whole.source.replace(/^parts\s*=\s*\[[^\n]*\]/m, `parts = ${JSON.stringify(parts)}`)
      : `parts = ${JSON.stringify(parts)}\n${whole.source}`;
    writeText(nativePath(directory, `${whole.name}.artf`), text);
  }
}

/** Declare the Artifact `name` in its own folder `repo/{folders...}` with the body `body`. */
export function declareArtifact(repo: NativePath, folders: string[], name: string, body: string): void {
  writeDeclaration(repo, nativePath(makeDirectory(nativePath(repo, ...folders)), "index.artf"), `name = ${JSON.stringify(name)}\n${body}`);
}

/** What the stand-in program does when a runtime eval or a Human tool runs it. */
export type Operation = "pass" | "fail" | "read" | "launch" | "talk" | "slow" | "hold" | "report" | "fingerprint" | "say";

/** The `command` and `args` fields that run the stand-in program with `operation`. */
export function commandFields(operation: Operation, ...args: string[]): string {
  const script = portablePath(suitePath("fixtures", "program.ts"));
  return `command = ${JSON.stringify(portablePath(nodeProgram))}, args = ${JSON.stringify([script, operation, ...args])}`;
}

/** A runtime eval's profile that runs the stand-in program. */
export function runtimeProfile(operation: Operation = "pass", timeoutMs?: number, ...args: string[]): string {
  return `{ kind = "runtime", ${commandFields(operation, ...args)}${timeoutMs === undefined ? "" : `, timeout_ms = ${timeoutMs}`} }`;
}

/** A runtime eval `id` titled `Check {name}` that runs the stand-in program. */
export function check(id: string, name: string, operation: Operation = "pass", instruction = `Check ${name}.`): string {
  return `\n[evals.${id}]\ntitle = "Check ${name}"\nprofile = ${runtimeProfile(operation)}\npayload.instruction = ${JSON.stringify(instruction)}\n`;
}
