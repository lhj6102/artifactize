// A fixture-tree builder that writes only named definitions. The tree in the fixture
// recipe is materialized as explicit parts, never interpreted by product discovery.
import { portablePath } from "./platform.ts";
import { makeDirectory, nativePath, parentOf, writeText, type NativePath } from "./portable.ts";

type Node = { name: string; target: string; source: string; declared: string[] };
const projects = new Map<NativePath, Map<string, Node>>();

export function writeDeclaration(repo: NativePath, targetFile: NativePath, source: string): void {
  const named = /^name\s*=\s*("(?:\\.|[^"\\])*"|'[^']*')\s*$/m.exec(source);
  if (!named) throw new Error("a fixture declaration needs its builder name");
  const name = named[1].startsWith('"') ? JSON.parse(named[1]) as string : named[1].slice(1, -1);
  const folder = portablePath(parentOf(targetFile)).slice(portablePath(repo).length).replace(/^\//, "");
  const target = folder ? `${folder}/` : "./";
  const declaredText = /^parts\s*=\s*(\[[^\n]*\])/m.exec(source)?.[1];
  const declared = declaredText ? JSON.parse(declaredText.replaceAll("'", '"')) as string[] : [];
  const node = { name, target, source: source.replace(named[0], `path = ${JSON.stringify(target)}`), declared };
  const nodes = projects.get(repo) ?? new Map<string, Node>();
  projects.set(repo, nodes); nodes.set(name, node);
  const directory = makeDirectory(nativePath(repo, ".artifactize"));
  makeDirectory(parentOf(targetFile));
  // Definitions no longer keep empty target folders in Git worktree copies.
  writeText(nativePath(parentOf(targetFile), ".gitkeep"), "");
  for (const whole of [...nodes.values()].filter((whole) => whole === node || whole.target === "./" || node.target.startsWith(whole.target))) {
    const children = [...nodes.values()].filter((part) => {
      if (part === whole || !(whole.target === "./" || part.target.startsWith(whole.target))) return false;
      return ![...nodes.values()].some((between) => between !== whole && between !== part && between.target.length > whole.target.length && (whole.target === "./" || between.target.startsWith(whole.target)) && part.target.startsWith(between.target));
    }).map((part) => part.name);
    const parts = [...new Set([...whole.declared, ...children])].sort();
    const text = parts.length ? /^parts\s*=/m.test(whole.source)
      ? whole.source.replace(/^parts\s*=\s*\[[^\n]*\]/m, `parts = ${JSON.stringify(parts)}`)
      : `parts = ${JSON.stringify(parts)}\n${whole.source}` : whole.source;
    writeText(nativePath(directory, `${whole.name}.artf`), text);
  }
}
