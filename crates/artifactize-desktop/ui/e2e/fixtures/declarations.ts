// Definitions a fixture writes into its repository, as `.artifactize/{name}.artf` files in the
// .artf language the CLI reads, and the stand-in program their runtime evals and Human tools run.
import { nodeProgram, portablePath } from "../platform.ts";
import { makeDirectory, nativePath, suitePath, writeText, type NativePath } from "../portable.ts";

/** One Artifact as a fixture declares it: the toolsets it composes (`files & release`), the
 * statements of its body (`basis = true`, `parts = [site]`, its evals) and the code its file
 * holds before it, such as its own toolset. Its `path`, and the parts its folder holds, come
 * from where it is declared. A string is a body alone. */
export type Definition = string | { tools?: string; body: string; beside?: string };

type Node = { name: string; target: string; tools: string; body: string; beside: string; declared: string[] };
const projects = new Map<NativePath, Map<string, Node>>();

/** `text` with each line indented by `by` spaces, an empty line left empty. */
export function indent(text: string, by = 2): string {
  return text.split("\n").map((line) => (line.trim() ? `${" ".repeat(by)}${line}` : "")).join("\n");
}

/** The code that binds `node`, with its parts `parts`. */
function code(node: Node, parts: string[]): string {
  const lines = [`path = ${JSON.stringify(node.target)}`, ...(parts.length ? [`parts = [${parts.join(", ")}]`] : []), node.body.trim()];
  const head = `artf ${node.name}${node.tools ? ` : ${node.tools}` : ""} = {`;
  return `${node.beside ? `${node.beside.trim()}\n\n` : ""}${head}\n${indent(lines.filter(Boolean).join("\n"))}\n}\n`;
}

/** Declare the Artifact `name` at `folder` in `repo`, as `definition` says: its definition gets
 * that folder as its `path`, and every Artifact whose folder holds it lists it among its parts,
 * as a project laid out in folders reads. */
export function writeDeclaration(repo: NativePath, folder: NativePath, name: string, definition: Definition): void {
  const { tools = "", body, beside = "" } = typeof definition === "string" ? { body: definition } : definition;
  const relative = portablePath(folder).slice(portablePath(repo).length).replace(/^\//, "");
  const target = relative ? `${relative}/` : "./";
  const parts = /^parts\s*=\s*\[([^\]\n]*)\]\s*$/m.exec(body);
  const declared = parts ? parts[1].split(",").map((part) => part.trim()).filter(Boolean) : [];
  const node: Node = { name, target, tools, body: parts ? body.replace(parts[0], "") : body, beside, declared };
  const nodes = projects.get(repo) ?? new Map<string, Node>();
  projects.set(repo, nodes);
  nodes.set(name, node);
  const directory = makeDirectory(nativePath(repo, ".artifactize"));
  makeDirectory(folder);
  // Git keeps no empty folder: a worktree of the repository still has every target.
  writeText(nativePath(folder, ".gitkeep"), "");
  const inside = (whole: Node, part: Node) => whole.target === "./" || part.target.startsWith(whole.target);
  for (const whole of nodes.values()) {
    if (whole !== node && !inside(whole, node)) continue;
    const children = [...nodes.values()].filter((part) => part !== whole && inside(whole, part) && ![...nodes.values()].some((between) =>
      between !== whole && between !== part && between.target.length > whole.target.length && inside(whole, between) && part.target.startsWith(between.target)));
    writeText(nativePath(directory, `${whole.name}.artf`), code(whole, [...new Set([...whole.declared, ...children.map((part) => part.name)])].sort()));
  }
}

/** Declare the Artifact `name` in its own folder `repo/{folders...}`. */
export function declareArtifact(repo: NativePath, folders: string[], name: string, definition: Definition): void {
  writeDeclaration(repo, makeDirectory(nativePath(repo, ...folders)), name, definition);
}

/** What the stand-in program does when a runtime eval or a Human tool runs it. */
export type Operation = "pass" | "fail" | "read" | "launch" | "talk" | "slow" | "hold" | "report" | "fingerprint" | "say";

/** The `command` and `args` arguments that run the stand-in program with `operation`. */
export function commandFields(operation: Operation, ...args: string[]): string {
  const script = portablePath(suitePath("fixtures", "program.ts"));
  return `command = ${JSON.stringify(portablePath(nodeProgram))}, args = ${JSON.stringify([script, operation, ...args])}`;
}

/** A runtime eval's evaluator that runs the stand-in program. */
export function runtimeProfile(operation: Operation = "pass", timeoutMs?: number, ...args: string[]): string {
  return `runtime(${commandFields(operation, ...args)}${timeoutMs === undefined ? "" : `, timeout_ms = ${timeoutMs}`})`;
}

/** An eval `id` run by `evaluator` (`human`, `runtime(…)` or a profile's name) with the statements `lines`. */
export function evalBlock(id: string, evaluator: string, lines: string[]): string {
  return `\neval ${id} : ${evaluator} = {\n${indent(lines.join("\n"))}\n}\n`;
}

/** A Human eval `id` titled `title` that asks `instruction`. */
export function humanEval(id: string, title: string, instruction: string): string {
  return evalBlock(id, "human", [`title = ${JSON.stringify(title)}`, `instruction = ${JSON.stringify(instruction)}`]);
}

/** A runtime eval `id` of Artifact `name`, titled `Check {name}`, that runs the stand-in program.
 * A `{other}` in `instruction` names an Artifact the check reads: it is one of its inputs, and
 * its instruction reads it as the name. */
export function check(id: string, name: string, operation: Operation = "pass", instruction = `Check ${name}.`): string {
  const named = /\{([A-Za-z][\w-]*)\}/g;
  const inputs = [...new Set([...instruction.matchAll(named)].map((match) => match[1]))].filter((input) => input !== name);
  return evalBlock(id, runtimeProfile(operation), [
    `title = ${JSON.stringify(`Check ${name}`)}`,
    `instruction = ${JSON.stringify(instruction.replace(named, "$1"))}`,
    ...(inputs.length ? [`inputs = [${inputs.join(", ")}]`] : []),
  ]);
}
