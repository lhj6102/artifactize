// Every edge id of the UX graph appears in the end-to-end tests, so a transition the graph
// promises cannot go untested unnoticed. Usage: node check-edges.ts UX-GRAPH.md [tests-directory]
import { absolutePath, nativePath, readText } from "./platform.ts";

const [graph, directory = "."] = process.argv.slice(2);
if (graph === undefined) {
  console.error("usage: check-edges.ts UX-GRAPH.md [tests-directory]");
  process.exit(2);
}
const ids = [...readText(absolutePath(graph)).matchAll(/^\|\s*(E-[A-Z0-9-]+)\s*\|/gm)].map((match) => match[1]);
if (ids.length === 0) {
  console.error(`${graph} has no edge ids`);
  process.exit(2);
}
// Check only modules actually registered in the WDIO catalog, never dead legacy tests.
const catalog = readText(absolutePath(directory, "catalog.ts"));
const sources = [...catalog.matchAll(/from\s+"(\.\/cases\/[^"]+\.ts)"/g)].map((match) => match[1]);
if (sources.length === 0) throw new Error("the desktop WDIO catalog has no scenario modules");
const tests = sources.map((name) => readText(nativePath(absolutePath(directory), name))).join("\n");
const missing = ids.filter((id) => !new RegExp(`(?<![A-Z0-9-])${id}(?![A-Z0-9-])`).test(tests));
console.log(`${ids.length - missing.length} of ${ids.length} edges appear in the tests`);
for (const id of missing) console.log(`missing: ${id}`);
process.exit(missing.length === 0 ? 0 : 1);
