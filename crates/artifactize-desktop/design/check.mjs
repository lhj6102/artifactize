// Mechanical checks of the desktop design Artifact. Usage:
//   node check.mjs page PAGE.html          one screen page: links the three sheets, has its
//                                          data-screen main, numbered callouts with notes,
//                                          unique D- rules each with a dd, no colour literal
//   node check.mjs whole [DIR]             the whole design: tokens in both schemes, every page
//                                          well formed, D- ids unique across pages, every screen
//                                          page listed on index.html and in ux-graph-doc.html,
//                                          every token named on index.html, no colour literal
//                                          outside tokens.css
//   node check.mjs doc DOC.html UX-GRAPH.md  the graph doc cites every edge of UX-GRAPH.md
//                                          exactly once, no other edge, and only known screens
//   node check.mjs tokens A.css B.css      the two sheets declare the same tokens with the
//                                          same values in the same scheme blocks
// Exit 0 when everything holds; 1 with one line per problem; 2 on a usage error.
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname, basename } from "node:path";

const SHEETS = ["tokens.css", "mock.css", "design.css"];
const SCREENS = ["monitor", "current", "detail", "review", "settings", "overlays"];
const TERMINALS = ["start", "any", "quit"];
const COLOUR = /#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(|(?<![-\w])(?:white|black)(?![-\w])/gi;
const read = (path) => readFileSync(path, "utf8");
const problems = [];
const fail = (text) => problems.push(text);

/** The custom properties a sheet declares, by scheme block: `root` and `light`. */
function tokens(css) {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const blocks = { root: new Map(), light: new Map() };
  const media = bare.match(/@media \(prefers-color-scheme: light\)\s*\{([\s\S]*?)\}\s*\}/);
  const rootOnly = media ? bare.replace(media[0], "") : bare;
  const take = (text, into) => {
    for (const match of text.matchAll(/--([a-z0-9-]+)\s*:\s*([^;]+);/g)) into.set(match[1], match[2].trim().replace(/\s+/g, " "));
  };
  take(rootOnly, blocks.root);
  if (media) take(media[1], blocks.light);
  return blocks;
}

/** Colour literals in `text` outside tokens.css: hex, rgb(), hsl(), white, black. */
function literals(text) {
  return [...text.matchAll(COLOUR)].map((match) => match[0]);
}

function page(path, { report = fail } = {}) {
  const html = read(path);
  const name = basename(path);
  for (const sheet of SHEETS) if (!html.includes(`href="${sheet}"`)) report(`${name}: does not link ${sheet}`);
  if (!/<title>[^<]+<\/title>/.test(html)) report(`${name}: has no title`);
  const main = html.match(/<main data-screen="([a-z-]+)">/);
  if (!main) report(`${name}: has no <main data-screen>`);
  const rules = [...html.matchAll(/<dt id="(D-[A-Z]+-\d+)">(D-[A-Z]+-\d+)<\/dt>\s*<dd>/g)];
  const ids = rules.map((match) => match[1]);
  for (const [, id, text] of rules) if (id !== text) report(`${name}: rule ${id} is labelled ${text}`);
  if (new Set(ids).size !== ids.length) report(`${name}: repeats a rule id`);
  const bareDts = [...html.matchAll(/<dt id="(D-[A-Z]+-\d+)"[^>]*>/g)].length;
  if (bareDts !== rules.length) report(`${name}: a D- rule has no <dd> right after it or carries extra attributes`);
  const callouts = [...html.matchAll(/<span class="callout"[^>]*>(\d+)<\/span>/g)].map((match) => match[1]);
  const notes = [...html.matchAll(/<span class="n">(\d+)<\/span>/g)].map((match) => match[1]);
  for (const number of callouts) if (!notes.includes(number)) report(`${name}: callout ${number} has no note`);
  for (const number of notes) if (!callouts.includes(number)) report(`${name}: note ${number} has no callout`);
  // Only what styles counts: style attributes and <style> blocks, not the prose.
  const styled = [...html.matchAll(/style="([^"]*)"/g), ...html.matchAll(/<style>([\s\S]*?)<\/style>/g)].map((match) => match[1]).join("\n");
  for (const literal of literals(styled)) report(`${name}: colour literal ${literal}`);
  if (main && SCREENS.includes(main[1]) && rules.length === 0) report(`${name}: a screen page has no D- rules`);
  return { screen: main?.[1] ?? null, ids };
}

function whole(dir) {
  const sheet = tokens(read(join(dir, "tokens.css")));
  const colourish = (value) => /#|rgb|hsl/.test(value);
  for (const [name, value] of sheet.root) if (colourish(value) && !sheet.light.has(name)) report(`tokens.css: --${name} has no light value`);
  for (const name of sheet.light.keys()) if (!sheet.root.has(name)) report(`tokens.css: --${name} is only in the light scheme`);
  for (const sheetName of ["mock.css", "design.css"]) for (const literal of literals(read(join(dir, sheetName)).replace(/\/\*[\s\S]*?\*\//g, ""))) report(`${sheetName}: colour literal ${literal}`);
  const pages = readdirSync(dir).filter((entry) => entry.endsWith(".html")).sort();
  const seen = new Map();
  const screens = new Map();
  for (const file of pages) {
    const { screen, ids } = page(join(dir, file));
    if (screen) screens.set(screen, file);
    for (const id of ids) {
      if (seen.has(id)) report(`${file}: rule ${id} is also in ${seen.get(id)}`);
      seen.set(id, file);
    }
  }
  const index = read(join(dir, "index.html"));
  const doc = read(join(dir, "ux-graph-doc.html"));
  for (const screen of SCREENS) {
    const file = `${screen}.html`;
    if (!pages.includes(file)) report(`whole: no page ${file}`);
    if (screens.get(screen) !== file) report(`whole: ${file} does not declare data-screen="${screen}"`);
    if (!index.includes(`href="${file}"`)) report(`index.html: does not link ${file}`);
    if (!doc.includes(`data-screen="${screen}"`)) report(`ux-graph-doc.html: does not list the screen ${screen}`);
  }
  for (const screen of screens.keys()) if (!SCREENS.includes(screen) && screen !== "ux-graph-doc" && screen !== "index") report(`whole: unknown screen ${screen}`);
  for (const name of sheet.root.keys()) if (!index.includes(`--${name}</code>`)) report(`index.html: does not name the token --${name}`);
  if (!index.includes('href="ux-graph-doc.html"')) report("index.html: does not link ux-graph-doc.html");
}

function report(text) {
  fail(text);
}

function doc(docPath, graphPath) {
  const html = read(docPath);
  const edges = [...read(graphPath).matchAll(/^\|\s*(E-[A-Z0-9-]+)\s*\|/gm)].map((match) => match[1]);
  if (edges.length === 0) fail(`${graphPath}: has no edge ids`);
  const rows = [...html.matchAll(/<tr id="(E-[A-Z0-9-]+)" data-edge="(E-[A-Z0-9-]+)" data-from="([a-z -]+)" data-to="([a-z -]+)">/g)];
  const cited = rows.map((match) => match[2]);
  for (const [, id, edge, from, to] of rows) {
    if (id !== edge) fail(`${edge}: row id ${id} differs`);
    for (const screen of [...from.split(/\s+/), ...to.split(/\s+/)]) if (!SCREENS.includes(screen) && !TERMINALS.includes(screen)) fail(`${edge}: unknown screen ${screen}`);
  }
  for (const edge of edges) {
    const times = cited.filter((id) => id === edge).length;
    if (times !== 1) fail(`${edge}: cited ${times} times in the doc`);
  }
  for (const edge of cited) if (!edges.includes(edge)) fail(`${edge}: not an edge of ${basename(graphPath)}`);
  for (const screen of [...SCREENS, ...TERMINALS]) {
    if (!html.includes(`data-screen="${screen}"`) && !html.includes(`data-terminal="${screen}"`)) fail(`doc: does not describe ${screen}`);
  }
}

function same(aPath, bPath) {
  const a = tokens(read(aPath));
  const b = tokens(read(bPath));
  for (const block of ["root", "light"]) {
    for (const [name, value] of a[block]) {
      if (!b[block].has(name)) fail(`${basename(bPath)}: ${block} lacks --${name}`);
      else if (b[block].get(name) !== value) fail(`${basename(bPath)}: ${block} --${name} is ${b[block].get(name)}, the design says ${value}`);
    }
    for (const name of b[block].keys()) if (!a[block].has(name)) fail(`${basename(bPath)}: ${block} adds --${name}, which the design does not define`);
  }
}

const [mode, ...rest] = process.argv.slice(2);
if (mode === "page" && rest[0]) page(rest[0]);
else if (mode === "whole") whole(rest[0] ?? dirname(new URL(import.meta.url).pathname));
else if (mode === "doc" && rest[0] && rest[1]) doc(rest[0], rest[1]);
else if (mode === "tokens" && rest[0] && rest[1]) same(rest[0], rest[1]);
else {
  console.error("usage: check.mjs page PAGE.html | whole [DIR] | doc DOC.html UX-GRAPH.md | tokens A.css B.css");
  process.exit(2);
}
for (const problem of problems) console.log(problem);
console.log(problems.length === 0 ? `${mode}: ok` : `${mode}: ${problems.length} problem(s)`);
process.exit(problems.length === 0 ? 0 : 1);
