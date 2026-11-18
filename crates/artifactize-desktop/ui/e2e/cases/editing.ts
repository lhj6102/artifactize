// Editing a branch's Current: an Artifact's Definition and Source in its Detail, New Artifact,
// Rename and Delete, places by hand and parts by dragging on the graph, and comments. Each case
// ends in what the window shows and what the definition files on disk hold.
import assert from "node:assert/strict";
import { git } from "../environment.ts";
import { prepareFixture } from "../build-fixtures.ts";
import { verifyAll, type Fixture } from "../fixture.ts";
import { runtimeProfile } from "../native-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { exists, listFiles, makeDirectory, nameOf, nativePath, primaryModifier, readText, relativeTextPath, writeText } from "../platform.ts";
import { detail, FIND, findField, graphShown, nodeAt, offCentre, openArtifact } from "../graph-view.ts";
import { MAIN, chooseChip, mainTitle, scanned, showRun } from "../pane.ts";
import { afterRender, fill, press } from "../keys.ts";
import { clickAt, pointerGapClick, visibleCenter } from "../pointer.ts";
import { buttonKey, dispatchInput, tab } from "../input.ts";
import { screenshotPixels } from "../rendering.ts";

const CURRENT = 'section[aria-label="Current"]';
const DETAIL = 'aside[aria-label="Detail"]';
const TOOLS = '[aria-label="Graph tools"]';

/** The comments of the editing fixture: app's, under its title row, and core's, longer than
 * the three lines a comment shows. */
const APP_NOTE = "The app is split next, once the kit settles.\nThe bundle follows.";
const CORE_LINES = ["Core needs a review.", "Only the happy path runs.", "The errors are untested", "Nor is the cache."];
const CORE_NOTE = CORE_LINES.join("\n");

/** What the editing fixture holds besides its Artifacts: `comments` adds app's and core's;
 * `nested` keeps kit's file in `.artifactize/lib/` with a comment, placed in app's box by hand;
 * `placed` places the top level by hand. */
type Options = { comments?: boolean; nested?: boolean; placed?: boolean };

/** The definitions of the editing fixture: app and bundle each hold kit, docs reads core. */
function definitions({ comments = false, nested = false, placed = false }: Options): Record<string, string> {
  const comment = (text: string, shown = comments) => (shown ? `\n[annotations.comment]\ntext = ${JSON.stringify(text)}\n` : "");
  const at = (x: number, y: number) => (placed ? `\n[annotations.style]\nat = [${x.toFixed(1)}, ${y.toFixed(1)}]\n` : "");
  return {
    app: `path = "app/"\nparts = ["kit"]\nbasis = true\n${at(0, 0)}${nested ? "\n[annotations.style.parts]\nkit = [0.0, 0.0]\n" : ""}${comment(APP_NOTE)}`,
    bundle: `path = "bundle/"\nparts = ["kit"]\nbasis = true\n${at(420, 0)}`,
    kit: `path = "kit/"\nbasis = true\n${comment("Kit is shared.", nested)}`,
    core: `path = "core/"\nbasis = true\n${at(0, 320)}${comment(CORE_NOTE)}`,
    docs: `path = "docs/"\n${at(420, 320)}\n[evals.check]\ntitle = "The docs read the core"\nprofile = ${runtimeProfile()}\npayload.instruction = "Check {docs} against {core}."\n`,
  };
}

/** A branch whose definitions are in `.artifactize/`, one file each, without a Run. */
function createEditing(options: Options = {}): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    const folder = makeDirectory(nativePath(repo, ".artifactize"));
    for (const [name, text] of Object.entries(definitions(options))) {
      writeText(nativePath(makeDirectory(nativePath(repo, name)), "readme.md"), `${name}\n`);
      const into = options.nested && name === "kit" ? makeDirectory(nativePath(folder, "lib")) : folder;
      writeText(nativePath(into, `${name}.artf`), text);
    }
    git(repo, "init", "-q", "--initial-branch=master", repo);
    return { root, repo, state: nativePath(root, "state") };
  });
}

/** The definition file of `name` as it is on disk now, in `.artifactize/` or its `lib/`, or
 * `null` once it is gone. */
const file = (fixture: Fixture, name: string) => {
  const path = [nativePath(fixture.repo, ".artifactize", `${name}.artf`), nativePath(fixture.repo, ".artifactize", "lib", `${name}.artf`)].find(exists);
  return path ? readText(path) : null;
};

/** Whether `name`'s file lists `part` among its parts. */
const holds = (fixture: Fixture, name: string, part: string) => (file(fixture, name) ?? "").split("\n").some((line) => line.startsWith("parts = [") && line.includes(`"${part}"`));

/** Whether `name`'s file places `part` in its box. */
const placesPart = (fixture: Fixture, name: string, part: string) => (file(fixture, name) ?? "").split("\n").some((line) => line.startsWith(`${part} = [`));

/** The comment `name`'s file holds, as its `[annotations.comment]` text: a basic string or a
 * multi-line one; `null` without one. */
function commentIn(fixture: Fixture, name: string): string | null {
  const text = file(fixture, name) ?? "";
  const section = text.indexOf("[annotations.comment]");
  if (section < 0) return null;
  const rest = text.slice(section).split("\n").slice(1).join("\n");
  const start = rest.indexOf("text = ");
  if (start < 0) return null;
  const value = rest.slice(start + "text = ".length);
  const DELIMITER = '"""';
  if (value.startsWith(DELIMITER)) {
    const body = value.slice(DELIMITER.length, value.indexOf(DELIMITER, DELIMITER.length));
    return body.startsWith("\n") ? body.slice(1) : body;
  }
  return JSON.parse(value.slice(0, value.indexOf("\n") < 0 ? undefined : value.indexOf("\n"))) as string;
}

/** A place written as `key = [x, y]` in `name`'s file: `at` for its own, or a part's name. */
function placeOf(fixture: Fixture, name: string, key: string): [number, number] | null {
  const line = (file(fixture, name) ?? "").split("\n").find((each) => each.startsWith(`${key} = [`));
  if (!line) return null;
  const [x, y] = line.slice(line.indexOf("[") + 1, line.indexOf("]")).split(",").map(Number);
  return [x, y];
}

/** `name`'s definition file on disk, in `.artifactize/` or its `lib/`. */
const pathOf = (fixture: Fixture, name: string) =>
  [nativePath(fixture.repo, ".artifactize", `${name}.artf`), nativePath(fixture.repo, ".artifactize", "lib", `${name}.artf`)].find(exists)!;

/** `name`'s own place as its file says, or `null`. */
const ownAt = (fixture: Fixture, name: string) => /^at = \[[^\]]*\]/m.exec(file(fixture, name) ?? "")?.[0] ?? null;

/** The shown graph's camera: its pan and zoom, as the canvas transforms what it draws. */
const camera = (browser: WebdriverIO.Browser) => browser.execute(() =>
  [...document.querySelectorAll<HTMLElement>(".svelte-flow__viewport")].find((view) => !view.closest("aside"))?.style.transform ?? "");

/** The drawn path of the first edge whose id names `id`. */
const edgeOf = (browser: WebdriverIO.Browser, id: string) => browser.execute((root, want) =>
  [...document.querySelectorAll(`${root} .svelte-flow__edge`)].find((edge) => edge.getAttribute("data-id")?.includes(want))?.querySelector("path")?.getAttribute("d") ?? "", CURRENT, id);

/** Whether the drawn copy `id` is being dragged, raised. */
const lifted = (browser: WebdriverIO.Browser, id: string) => browser.execute((selector) => {
  const node = document.querySelector(selector);
  const face = node?.querySelector(".artifact, .whole");
  return !!node?.classList.contains("dragging") && !!face && getComputedStyle(face).boxShadow !== "none";
}, `${CURRENT} ${nodeAt(id)}`);

/** The comment drawn under or inside the copy `id`: its box, its node's box, its colours, and
 * whether its text is cut to the lines it shows. */
const commentOf = (browser: WebdriverIO.Browser, id: string) => browser.execute((selector) => {
  const node = document.querySelector<HTMLElement>(selector);
  const text = node?.querySelector<HTMLElement>(".comment");
  if (!node || !text) return null;
  const [at, around, style] = [text.getBoundingClientRect(), node.getBoundingClientRect(), getComputedStyle(text)];
  // The token as the computed colour reads it: `#rrggbb` as `rgb(r, g, b)`.
  const hex = getComputedStyle(document.documentElement).getPropertyValue("--ink-2").trim();
  const quiet = `rgb(${[1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16)).join(", ")})`;
  return {
    box: { left: at.left, top: at.top, right: at.right, bottom: at.bottom, height: at.height - Number.parseFloat(style.paddingTop) - Number.parseFloat(style.paddingBottom) },
    node: { left: around.left, top: around.top, right: around.right, bottom: around.bottom },
    quiet: style.color === quiet,
    bare: style.backgroundColor === "rgba(0, 0, 0, 0)" && style.borderTopStyle === "none",
    cut: text.scrollHeight > text.clientHeight + 1,
    lineHeight: Number.parseFloat(style.lineHeight),
  };
}, `${CURRENT} ${nodeAt(id)}, .run ${nodeAt(id)}`);

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` }).catch(async (error: Error) => {
    // What the window shows, to say why it did not come.
    const shown = await browser.execute(() => ({
      notice: document.querySelector(".notice")?.textContent?.trim() ?? "",
      alerts: [...document.querySelectorAll('[role="alert"]')].map((alert) => alert.textContent?.trim() ?? ""),
      focused: document.activeElement?.outerHTML.slice(0, 160) ?? "",
    }));
    throw new Error(`${error.message}; the window shows ${JSON.stringify(shown)}`);
  });
}

/** The Current of the fixture's branch shown, its graph drawn with `id`. */
async function currentShown(browser: WebdriverIO.Browser, id: string) {
  await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current of the branch");
  await waitFor(browser, async () => /^Scanned at \d/.test(await scanned(browser)), "its scan ended");
  await graphShown(browser, id);
}

/** The Main area's notice: why an edit on the graph wrote nothing. */
const notice = (browser: WebdriverIO.Browser) => browser.execute((main) => document.querySelector(`${main} .notice`)?.textContent?.trim() ?? "", MAIN);

/** The box of the drawn copy `id`, on the Current's graph, in the window. */
const box = (browser: WebdriverIO.Browser, id: string) => browser.execute((selector) => {
  const found = document.querySelector<HTMLElement>(selector);
  if (!found) return null;
  const { left, top, width, height } = found.getBoundingClientRect();
  return { left, top, width, height, x: left + width / 2, y: top + height / 2 };
}, `${CURRENT} ${nodeAt(id)}, .run ${nodeAt(id)}`);

function partDrawn(browser: WebdriverIO.Browser, id: string, whole: string, inside: boolean): Promise<boolean> {
  return browser.execute((selectors, inside) => {
    const [part, container, canvas] = selectors.map((selector) => document.querySelector<HTMLElement>(selector));
    if (!part?.checkVisibility({ visibilityProperty: true }) || !container?.checkVisibility({ visibilityProperty: true }) || !canvas) return false;
    const bounds = part.getBoundingClientRect();
    const parent = container.getBoundingClientRect();
    const view = canvas.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0 || bounds.right <= view.left || bounds.left >= view.right
      || bounds.bottom <= view.top || bounds.top >= view.bottom) return false;
    return inside ? bounds.left >= parent.left - 1 && bounds.top >= parent.top - 1
      && bounds.right <= parent.right + 1 && bounds.bottom <= parent.bottom + 1
      : bounds.right <= parent.left + 1 || bounds.left >= parent.right - 1 || bounds.bottom <= parent.top + 1 || bounds.top >= parent.bottom - 1;
  }, [`${CURRENT} ${nodeAt(id)}`, `${CURRENT} ${nodeAt(whole)}`, `${CURRENT} .canvas`], inside);
}

/** Each drawn copy's CSS placement relative to docs, before the outer viewport's pan and zoom.
 * Reading the node's transform avoids rounding screen pixels back into graph units. */
const places = (browser: WebdriverIO.Browser) => browser.execute(() => {
  const nodes = [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].filter((node) => !node.closest("aside"));
  const position = (node: HTMLElement) => new DOMMatrixReadOnly(getComputedStyle(node).transform);
  const docs = nodes.find((node) => node.dataset.id === "a:docs");
  const origin = docs ? position(docs) : new DOMMatrixReadOnly();
  return Object.fromEntries(nodes.map((node) => {
    const at = position(node);
    return [node.dataset.id ?? "", [Math.round(at.e - origin.e), Math.round(at.f - origin.f)]];
  }));
});

/** An exposed point of the Current's canvas below its nodes when there is room,
 * otherwise on nearby graph background. A moved node can extend past the viewport. */
const below = (browser: WebdriverIO.Browser, x: number, originY?: number) => browser.execute((root, want, originY) => {
  const canvas = document.querySelector(`${root} .canvas`)!;
  const bounds = canvas.getBoundingClientRect();
  const nodes = [...canvas.querySelectorAll(".svelte-flow__node")].map((node) => node.getBoundingClientRect());
  const left = Math.ceil(Math.max(bounds.left, 0)) + 8, right = Math.floor(Math.min(bounds.right, innerWidth)) - 8;
  const top = Math.ceil(Math.max(bounds.top, 0)) + 8, bottom = Math.floor(Math.min(bounds.bottom, innerHeight)) - 8;
  const x = Math.max(left, Math.min(right, Math.round(want)));
  const nodesBottom = Math.max(...nodes.map((node) => node.bottom));
  const isFree = (x: number, y: number) => {
    const hit = document.elementFromPoint(x, y);
    return !!hit && canvas.contains(hit) && !hit.closest(".svelte-flow__node, button, aside, .comment")
      && (originY === undefined || Math.hypot(x - want, y - originY) > 16);
  };
  if (nodesBottom < bottom) {
    const y = Math.round((Math.max(top, nodesBottom) + bottom) / 2);
    if (isFree(x, y)) return { x, y };
  }
  for (let y = bottom; y >= top; y -= 12) {
    for (let dx = 0; dx <= right - left; dx += 12) {
      for (const next of [x - dx, x + dx]) if (next >= left && next <= right && isFree(next, y)) return { x: next, y };
    }
  }
  throw new Error("no exposed background of the Current is reachable for the drop");
}, CURRENT, x, originY);

/** The box of the drawn copy `id` once the graph holds still: the same over two frames. */
async function settled(browser: WebdriverIO.Browser, id: string) {
  let last = "";
  let shown: Awaited<ReturnType<typeof box>> = null;
  await waitFor(browser, async () => {
    shown = await box(browser, id);
    await afterRender(browser);
    const now = JSON.stringify(shown);
    const still = now === last && shown !== null;
    last = now;
    return still;
  }, `${id} holds still`);
  return shown!;
}

/** Whether the drawn copy `id` is outlined as the drop target. */
const targeted = (browser: WebdriverIO.Browser, id: string) => browser.execute((selector) => !!document.querySelector(`${selector} .artifact.target, ${selector} .whole.target`), `${CURRENT} ${nodeAt(id)}`);

/** Press at `from`, move to `to` in steps and release there, through the suite's one input
 * helper: the embedded provider has no pointerdown or pointerup of its own. `held` runs while
 * the button is still down; `escape` presses Esc then. */
async function drag(
  browser: WebdriverIO.Browser,
  from: { x: number; y: number },
  to: { x: number; y: number },
  { held, escape = false, escaped }: { held?: () => Promise<void>; escape?: boolean; escaped?: () => Promise<void> } = {},
) {
  const at = (point: { x: number; y: number }) => ({ x: Math.round(point.x), y: Math.round(point.y) });
  await dispatchInput(browser, { kind: "pointer", phase: "down", ...at(from) });
  for (let step = 1; step <= 8; step += 1) {
    await dispatchInput(browser, { kind: "pointer", phase: "move", ...at({ x: from.x + ((to.x - from.x) * step) / 8, y: from.y + ((to.y - from.y) * step) / 8 }) });
  }
  await afterRender(browser);
  if (held) await held();
  // A key the provider drops is sent again; the page logs what arrives.
  if (escape) await press(browser, ["Escape"], () => true, "Esc during the drag");
  if (escaped) await escaped();
  await dispatchInput(browser, { kind: "pointer", phase: "up", ...at(to) });
}

/** The title row of the whole `id`'s box, which drags it. */
const titleOf = async (browser: WebdriverIO.Browser, id: string) =>
  visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt(id)} .title .name`).getElement());

/** The text of the open Detail's Definition, Path field included. */
const definition = (browser: WebdriverIO.Browser) => browser.execute((root) => {
  const section = [...document.querySelectorAll<HTMLElement>(`${root} .section`)].find((shown) => shown.querySelector(".head")?.textContent?.trim() === "Definition");
  if (!section) return null;
  const facts = Object.fromEntries([...section.querySelectorAll("dt")].map((term) => [term.textContent?.trim() ?? "", term.nextElementSibling as HTMLElement]));
  return {
    file: facts.File?.textContent?.trim() ?? "",
    path: facts.Path?.querySelector("input")?.value ?? "",
    missing: facts.Path?.querySelector(".refusal")?.textContent?.trim() ?? "",
    parts: [...(facts.Parts?.querySelectorAll(".part") ?? [])].map((part) => part.textContent?.trim() ?? ""),
    refused: [...section.querySelectorAll('[role="alert"]')].map((alert) => alert.textContent?.trim() ?? ""),
  };
}, DETAIL);

/** The Source of the open Detail: what its field holds, whether Save is offered, and what it
 * refused. */
const source = (browser: WebdriverIO.Browser) => browser.execute((root) => {
  const field = document.querySelector<HTMLTextAreaElement>(`${root} textarea[aria-label^="Source of "]`);
  const section = field?.closest(".section");
  return field && section ? {
    text: field.value,
    save: [...section.querySelectorAll("button")].some((button) => button.textContent?.trim() === "Save"),
    refused: section.querySelector('[role="alert"]')?.textContent?.trim() ?? "",
    errorColour: (() => {
      const alert = section.querySelector('[role="alert"]');
      if (!alert) return false;
      const hex = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
      return getComputedStyle(alert).color === `rgb(${[1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16)).join(", ")})`;
    })(),
  } : null;
}, DETAIL);

/** Unfold the open Detail's section `name` when it is folded. */
async function unfold(browser: WebdriverIO.Browser, name: string) {
  const head = browser.$(`${DETAIL} section[aria-label="${name}"] button.head`);
  await head.waitForExist({ timeout: 15000, timeoutMsg: `the Detail has ${name}` });
  if ((await head.getAttribute("aria-expanded")) !== "true") await head.click();
  await waitFor(browser, async () => (await head.getAttribute("aria-expanded")) === "true", `${name} unfolded`);
}

/** Type `text` after what the field `selector` holds. */
async function typeIn(browser: WebdriverIO.Browser, selector: string, text: string) {
  await fill(browser, selector, `${await browser.$(selector).getValue()}${text}`);
}

/** The comments drawn under the Current's nodes, by drawn copy. */
const comments = (browser: WebdriverIO.Browser) => browser.execute((root) => Object.fromEntries(
  [...document.querySelectorAll<HTMLElement>(root)]
    .map((node) => [node.dataset.id ?? "", node.querySelector(".comment")?.textContent?.trim() ?? ""])
    .filter(([, text]) => text !== ""),
), `${CURRENT} .svelte-flow__node, .run .svelte-flow__node`);

/** The comment field open on the graph: whose, and what it holds; focused or not. */
const field = (browser: WebdriverIO.Browser) => browser.execute(() => {
  const open = document.querySelector<HTMLTextAreaElement>('textarea[aria-label^="Comment on "]');
  if (!open) return null;
  const node = open.closest<HTMLElement>(".svelte-flow__node");
  const at = open.getBoundingClientRect(), above = node?.getBoundingClientRect();
  return { of: open.getAttribute("aria-label")?.replace("Comment on ", "") ?? "", text: open.value, focused: document.activeElement === open,
    under: above && at.top >= above.bottom - 1 && Math.abs(at.left - above.left) <= 1 ? node?.dataset.id : null };
});

export const scenarios: Scenario[] = [
  // UX journeys: J-DEFINITION J-DEFINITION-EDIT J-PARTS-EDIT J-DEFINITION-CONFLICT
  scenario<Record<string, never>>(
    "editing-definition",
    "shows and edits an Artifact's Definition and Source on the Current, refusing what the project would not accept, a changed file and a drag over typed text",
    async () => {
      const fixture = createEditing();
      writeText(pathOf(fixture, "app"), `${file(fixture, "app")}\n[annotations.comment]\ntext = ${JSON.stringify(APP_NOTE)}\n`);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await currentShown(browser, "a:core");
      // The Definition stands where a Run's Path does: the file, its PATH as written, its parts.
      await openArtifact(browser, "a:core", "Artifact core");
      await waitFor(browser, async () => (await definition(browser))?.file === ".artifactize/core.artf", "core's Definition names its file");
      assert.deepEqual(await definition(browser), { file: ".artifactize/core.artf", path: "core/", missing: "", parts: [], refused: [] });
      assert.equal(await browser.$(`${DETAIL} section[aria-label="Path"]`).isExisting(), false, "no Path section on a Current");
      await unfold(browser, "Source");
      assert.equal((await source(browser))?.text, file(fixture, "core"), "the Source is the file's text");

      // A Path is written by Enter; a missing target is said beside it.
      const path = `${DETAIL} input[aria-label="Path of core"]`;
      await fill(browser, path, "gone/");
      await browser.keys("Enter");
      await waitFor(browser, () => file(fixture, "core")?.includes('path = "gone/"') ?? false, "core's file holds the new Path");
      await waitFor(browser, async () => (await definition(browser))?.missing === "gone/ does not exist.", "the missing target said beside the Path");
      // Esc puts a Path being typed back, writing nothing.
      await fill(browser, path, "elsewhere/");
      await browser.keys("Escape");
      await waitFor(browser, async () => (await definition(browser))?.path === "gone/", "the Path put back");
      assert.equal(await browser.$(DETAIL).isExisting(), true, "Esc in the Path leaves the Detail open");
      await fill(browser, path, "core/");
      await browser.keys("Enter");
      await waitFor(browser, () => file(fixture, "core")?.includes('path = "core/"') ?? false, "core's Path back");
      // A Path the project would not accept is refused in red under it, writing nothing; it stays
      // typed to be corrected, and Esc puts it back.
      const kept = file(fixture, "core");
      await fill(browser, path, "../outside/");
      await browser.keys("Enter");
      await waitFor(browser, async () => ((await definition(browser))?.refused.length ?? 0) > 0, "the outside Path refused under it");
      assert.equal(file(fixture, "core"), kept, "a refused Path writes nothing");
      assert.equal((await definition(browser))?.path, "../outside/", "the Path stays typed");
      assert.equal(await browser.execute((field) => {
        // The token as the computed colour reads it: `#rrggbb` as `rgb(r, g, b)`.
        const hex = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
        const red = `rgb(${[1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16)).join(", ")})`;
        return getComputedStyle(document.querySelector(field)!.closest("dd")!.querySelector('[role="alert"]')!).color === red;
      }, path), true, "the refusal in red");
      await browser.keys("Escape");
      await waitFor(browser, async () => (await definition(browser))?.path === "core/" && (await definition(browser))?.refused.length === 0, "Esc puts the Path back, its refusal gone");

      // Typing in the Source offers Save and Revert; Ctrl+S writes it.
      const text = `${DETAIL} textarea[aria-label="Source of core"]`;
      await typeIn(browser, text, "\n# reviewed");
      await waitFor(browser, async () => (await source(browser))?.save === true, "Save offered for typed text");
      assert.equal(file(fixture, "core")?.includes("# reviewed"), false, "nothing written before Save");
      await press(browser, [primaryModifier, "s"], () => file(fixture, "core")?.endsWith("# reviewed") ?? false, "Ctrl+S wrote the Source");
      await waitFor(browser, async () => (await source(browser))?.save === false, "Save gone once written");
      // A change of what core is, saved from the Source, reaches its Definition and the graph.
      await typeIn(browser, text, '\nparts = ["kit"]');
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, () => holds(fixture, "core", "kit"), "core's file names kit as its part");
      await waitFor(browser, async () => JSON.stringify((await definition(browser))?.parts) === '["kit"]', "core's Definition lists kit");
      await waitFor(browser, () => partDrawn(browser, "a:core/a:kit", "a:core", true), "the graph draws kit inside core's visible box");
      await browser.$(`${DETAIL} button[aria-label="Remove kit from the parts of core"]`).click();
      await waitFor(browser, () => !holds(fixture, "core", "kit"), "kit gone from core's parts");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:core")} .artifact`).isDisplayed()
        && !(await browser.$(`${CURRENT} ${nodeAt("a:core/a:kit")}`).isDisplayed()), "core drawn as a visible standalone node again");
      // Text that does not read is refused at the actual bad character, in red.
      const readable = file(fixture, "core")!;
      const badLine = "path = @";
      const draft = `${readable}\n${badLine}`;
      await typeIn(browser, text, `\n${badLine}`);
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, async () => {
        const shown = await source(browser);
        return !!shown?.errorColour && shown.text === draft && shown.refused.includes(`line ${draft.split("\n").length}, column ${badLine.indexOf("@") + 1}`);
      }, "the same Source shows the red parse error at @ and keeps the complete draft");
      assert.equal(file(fixture, "core"), readable, "nothing written");
      await browser.$(DETAIL).$("button=Revert").click();
      await waitFor(browser, async () => (await source(browser))?.save === false, "Revert puts back the file");
      // What the project would not accept is refused with its line, and stays typed.
      const before = file(fixture, "core");
      await typeIn(browser, text, '\nparts = ["nobody"]');
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, async () => /line \d/.test((await source(browser))?.refused ?? ""), "the refusal names its line");
      assert.equal(file(fixture, "core"), before, "an invalid Source writes nothing");
      assert.ok((await source(browser))?.text.includes("nobody"), "the typed text stays to be corrected");
      // Revert puts back the file's text.
      await browser.$(DETAIL).$("button=Revert").click();
      await waitFor(browser, async () => (await source(browser))?.text === file(fixture, "core") && !(await source(browser))?.save, "Revert puts back the file");

      // Parts: the field suggests Artifacts; Enter adds one, its button removes it.
      await openArtifact(browser, "a:app", "Artifact app");
      await waitFor(browser, async () => JSON.stringify((await definition(browser))?.parts) === '["kit"]', "app's parts listed");
      await browser.$(DETAIL).$("button=Add").click();
      const adding = `${DETAIL} input[aria-label="Add a part to app"]`;
      await browser.$(adding).waitForExist({ timeout: 15000, timeoutMsg: "the Parts field opens" });
      assert.equal(await browser.$(adding).isFocused(), true, "the Parts field takes focus");
      await browser.keys("core");
      await waitFor(browser, async () => (await browser.$$(`${DETAIL} [role="option"]`).map((option) => option.getText())).join() === "core", "core suggested");
      await browser.keys("Enter");
      await waitFor(browser, () => /parts = \[[^\]]*"core"/.test(file(fixture, "app") ?? ""), "app's file names core as a part");
      await waitFor(browser, () => partDrawn(browser, "a:app/a:core", "a:app", true), "the graph draws core inside app's visible box");
      await browser.$(`${DETAIL} button[aria-label="Remove core from the parts of app"]`).click();
      await waitFor(browser, () => !/"core"/.test(file(fixture, "app") ?? ""), "core gone from app's parts");
      await waitFor(browser, async () => await partDrawn(browser, "a:core", "a:app", false)
        && !(await browser.$(`${CURRENT} ${nodeAt("a:app/a:core")}`).isDisplayed()), "the graph draws standalone core outside app's box");
      // A part that would make a cycle is refused under Parts; Esc closes the field.
      await openArtifact(browser, "a:app/a:kit", "Artifact kit");
      const kitBefore = file(fixture, "kit");
      await browser.$(DETAIL).$("button=Add").click();
      await browser.$(`${DETAIL} input[aria-label="Add a part to kit"]`).waitForExist({ timeout: 15000 });
      await browser.keys("app");
      await browser.keys("Enter");
      await waitFor(browser, async () => ((await definition(browser))?.refused ?? []).some((text) => /cycle|part of/i.test(text)), "the cycle refused under Parts");
      assert.equal(file(fixture, "kit"), kitBefore, "a refused part writes nothing");
      await browser.keys("Escape");
      await waitFor(browser, async () => !(await browser.$(`${DETAIL} input[aria-label="Add a part to kit"]`).isExisting()), "Esc closes the Parts field");

      // The top level placed by hand: Reset layout is offered.
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed");
      const docs = (await box(browser, "a:docs"))!;
      await drag(browser, docs, await below(browser, docs.x));
      await waitFor(browser, () => ownAt(fixture, "core") !== null && ownAt(fixture, "docs") !== null, "the top level placed in each file");

      // A file changed on disk refuses a save: the refusal names it, the typed text stays until
      // Revert shows what is on disk.
      await openArtifact(browser, "a:core", "Artifact core");
      await unfold(browser, "Source");
      await typeIn(browser, text, "\n# mine");
      const theirs = `${file(fixture, "core")}\n# theirs`;
      writeText(nativePath(fixture.repo, ".artifactize", "core.artf"), theirs);
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, async () => (await source(browser))?.refused === ".artifactize/core.artf changed on disk.", "the changed file refused");
      assert.equal(file(fixture, "core"), theirs, "the other writer's text stays");
      assert.ok((await source(browser))?.text.endsWith("# mine"), "the typed text stays");
      // Every edit on the graph that would write core's file is refused before anything is
      // written, naming the file; what was dragged goes back and the typed text stays with core
      // while its Detail is closed.
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed");
      const all = () => ["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name));
      let unsaved = all();
      // Before each refused edit a move of docs, which has no typed text, is written and clears
      // the notice: each refusal is the answer to its own edit.
      const fresh = async () => {
        const [docs, at] = [(await box(browser, "a:docs"))!, ownAt(fixture, "docs")];
        await drag(browser, docs, { x: docs.x + 30, y: docs.y });
        await waitFor(browser, async () => ownAt(fixture, "docs") !== at && (await notice(browser)) === "", "a move of docs written, the notice cleared");
        unsaved = all();
      };
      const typed = async (what: string) => {
        await waitFor(browser, async () => (await notice(browser)) === ".artifactize/core.artf has typed text in its Source; save or revert it first.", what);
        assert.deepEqual(all(), unsaved, `${what}: nothing written`);
      };
      const core = (await box(browser, "a:core"))!;
      await drag(browser, { x: core.x, y: core.y }, { x: core.x + 60, y: core.y + 40 });
      await typed("a move over typed text refused");
      assert.deepEqual(await box(browser, "a:core"), core, "core back where it was");
      await fresh();
      const app = (await box(browser, "a:app"))!;
      await drag(browser, core, { x: app.left + 60, y: app.top + 14 });
      await typed("a drop into app's box refused");
      assert.equal(await browser.$(`${CURRENT} ${nodeAt("a:app/a:core")}`).isExisting(), false, "core not drawn in app's box");
      assert.deepEqual(await box(browser, "a:core"), core, "core back again");
      await fresh();
      await browser.$(TOOLS).$("button=Reset layout").click();
      await typed("Reset layout refused");
      await fresh();
      const appComment = browser.$(`${CURRENT} ${nodeAt("a:app")} .comment`);
      await appComment.waitForDisplayed({ timeout: 15000 });
      const coreTarget = await settled(browser, "a:core");
      await drag(browser, await visibleCenter(browser, await appComment.getElement()), coreTarget, {
        held: () => waitFor(browser, () => targeted(browser, "a:core"), "the comment would drop onto core"),
      });
      await typed("a comment drop over typed text refused");
      await waitFor(browser, async () => (await comments(browser))["a:app"] === APP_NOTE && !(await comments(browser))["a:core"], "the comment returned visibly to app");
      await fresh();
      await openArtifact(browser, "a:core", "Artifact core");
      await browser.$(DETAIL).$("button=Comment").click();
      await waitFor(browser, async () => (await field(browser))?.of === "core", "core's comment field open");
      await browser.keys("Later");
      await browser.keys("Enter");
      await typed("a comment on core refused");
      assert.equal((await field(browser))?.text, "Later", "the comment field stays with its text");
      await browser.keys("Escape");
      await waitFor(browser, async () => (await field(browser)) === null, "Esc closes the comment field");
      await unfold(browser, "Source");
      await waitFor(browser, async () => (await source(browser))?.text.endsWith("# mine") ?? false, "the typed text still in core's Source");
      await browser.$(DETAIL).$("button=Revert").click();
      await waitFor(browser, async () => (await source(browser))?.text === theirs, "Revert shows what is on disk");
      // Saved, the next move is written.
      await typeIn(browser, text, "\n# ours");
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, () => file(fixture, "core")?.endsWith("# ours") ?? false, "the Source saved on what is on disk");
      await waitFor(browser, async () => {
        const shown = await source(browser);
        return shown !== null && !shown.save && shown.refused === "" && shown.text === file(fixture, "core");
      }, "the Source shows the saved file, nothing left to save");
      // An eval added in docs's Source: the Current is scanned again and docs's open Detail lists
      // it as what verify would do now.
      await openArtifact(browser, "a:docs", "Artifact docs");
      await unfold(browser, "Source");
      const docsText = `${DETAIL} textarea[aria-label="Source of docs"]`;
      const evals = () => browser.execute((root) => document.querySelector(`${root} section[aria-label="Evals"]`)?.textContent?.replace(/\s+/g, " ") ?? "", DETAIL);
      assert.equal((await evals()).includes("lint"), false, "docs has no lint yet");
      await typeIn(browser, docsText, `\n[evals.lint]\ntitle = "The docs are linted"\nprofile = ${runtimeProfile()}\npayload.instruction = "Lint {docs}."\n`);
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, async () => /\blint\b.*would run/.test(await evals()), `docs's Detail lists lint as would run: ${await evals()}`);
      assert.equal((await detail(browser))?.title, "Artifact docs", "docs's Detail stayed open");
    },
  ),

  // UX journeys: J-NODE-NEW J-NODE-RENAME J-NODE-DELETE J-DEFINITION-CONFLICT
  scenario<Record<string, never>>(
    "editing-nodes",
    "creates an Artifact from the tool strip, renames one with its references, and deletes one only while nothing names it",
    async () => {
      const fixture = createEditing({ nested: true, placed: true });
      writeText(pathOf(fixture, "app"), file(fixture, "app")!.replace("kit = [0.0, 0.0]", "kit = [64.0, 48.0]"));
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await currentShown(browser, "a:core");
      const dialog = () => browser.$('[role="dialog"]');
      const refusal = () => dialog().$('[role="alert"]');
      const under = (field: string) => dialog().$(`${field} + [role="alert"]`);
      const explains = async (field: string, reason: RegExp) => await under(field).isDisplayed() && reason.test(await under(field).getText());
      const definitionFiles = () => [nativePath(fixture.repo, ".artifactize"), nativePath(fixture.repo, ".artifactize", "lib")]
        .map((directory) => listFiles(directory).map((path) => [nameOf(path), readText(path)]).sort());
      const definitionsBefore = definitionFiles();
      // New Artifact: a name already used, ignoring case, is refused under its field.
      await browser.$(TOOLS).$("button=New Artifact").click();
      await dialog().waitForExist({ timeout: 15000, timeoutMsg: "the New Artifact dialog" });
      assert.equal(await dialog().$(".dialog-title").getText(), "New Artifact");
      const name = dialog().$("label:nth-of-type(1) input");
      const path = dialog().$("label:nth-of-type(2) input");
      await waitFor(browser, () => name.isFocused(), "Name gets focus when New Artifact opens");
      assert.match(await dialog().$("label:nth-of-type(1)").getText(), /Name/);
      assert.match(await dialog().$("label:nth-of-type(2)").getText(), /Path/);
      assert.ok((await path.getLocation("y")) >= (await name.getLocation("y")) + (await name.getSize("height")), "Path is below Name");
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "Core");
      await fill(browser, '[role="dialog"] label:nth-of-type(2) input', "core/");
      await dialog().$("button=Create").click();
      await waitFor(browser, () => explains("label:nth-of-type(1)", /core.*already exists/i), "the duplicate name explanation is under Name");
      assert.deepEqual(definitionFiles(), definitionsBefore, "refusal leaves the actual definition entries and contents unchanged");
      const invalid = (at: 1 | 2) => dialog().$(`label:nth-of-type(${at}) input`).getAttribute("aria-invalid");
      assert.equal(await invalid(1), "true", "the used name's field marked");
      // A name that is no name is refused under the Name; a Path outside the project under the Path.
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "two words");
      await dialog().$("button=Create").click();
      await waitFor(browser, async () => (await invalid(1)) === "true" && await explains("label:nth-of-type(1)", /name.*(?:invalid|must|expected)|invalid.*name/i), "a name with a space refused under the Name");
      assert.equal(file(fixture, "two words"), null, "nothing written");
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "outside");
      await fill(browser, '[role="dialog"] label:nth-of-type(2) input', "../outside/");
      await dialog().$("button=Create").click();
      await waitFor(browser, async () => (await invalid(2)) === "true" && (await invalid(1)) === "false" && await explains("label:nth-of-type(2)", /outside|project-relative|escape|\.\./i), "a Path outside the project refused under the Path");
      assert.equal(file(fixture, "outside"), null, "nothing written");
      await browser.keys("Escape");
      await dialog().waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "Esc closes the dialog" });
      // Created: the file is written, the dialog closes, the node is drawn in free space beside
      // the hand-placed top level, and its Detail opens with it in the middle of the graph left
      // of the Detail, at a zoom that reads.
      await browser.$(TOOLS).$("button=New Artifact").click();
      await dialog().waitForExist({ timeout: 15000 });
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "extra");
      await fill(browser, '[role="dialog"] label:nth-of-type(2) input', "extra/");
      await browser.keys("Enter");
      await waitFor(browser, () => file(fixture, "extra") !== null, "extra's file written");
      assert.ok(file(fixture, "extra")?.includes('path = "extra/"'), "with its Path");
      await dialog().waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "the dialog closed" });
      await graphShown(browser, "a:extra");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact extra", "the new Artifact's Detail open");
      await waitFor(browser, async () => {
        const at = await offCentre(browser, "a:extra");
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && at.width >= 168 * 0.75 - 1;
      }, "extra in the middle of the graph left of the Detail, at a zoom that reads");
      const overlaps = await browser.execute((root) => {
        const rects = [...document.querySelectorAll<HTMLElement>(`${root} .svelte-flow__node`)].map((node) => [node.dataset.id ?? "", node.getBoundingClientRect()] as const);
        const extra = rects.find(([id]) => id === "a:extra")![1];
        return rects.filter(([id, at]) => id !== "a:extra" && at.left < extra.right && extra.left < at.right && at.top < extra.bottom && extra.top < at.bottom).map(([id]) => id);
      }, CURRENT);
      assert.deepEqual(overlaps, [], "extra covers no Artifact");

      // A rename whose files changed on disk meanwhile is refused, writing none of them; the
      // graph and the Definition show what is on disk now.
      const docsTheirs = (file(fixture, "docs") ?? "").replace('path = "docs/"\n', 'path = "docs/"\nparts = ["kit"]\n');
      writeText(nativePath(fixture.repo, ".artifactize", "docs.artf"), docsTheirs);
      await openArtifact(browser, "a:core", "Artifact core");
      await browser.$(DETAIL).$("button=Rename").click();
      await dialog().waitForExist({ timeout: 15000 });
      await fill(browser, '[role="dialog"] input', "base");
      await dialog().$("button=Rename").click();
      await waitFor(browser, async () => (await refusal().isExisting()) && (await refusal().getText()) === ".artifactize/docs.artf changed on disk.", "the rename refused, naming docs's file");
      assert.equal(file(fixture, "base"), null, "no file renamed");
      assert.notEqual(file(fixture, "core"), null, "core's file stays");
      assert.equal(file(fixture, "docs"), docsTheirs, "docs's file as the other writer left it");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:docs/a:kit")}`).isExisting(), "the graph scanned again: kit in docs's box");
      await browser.keys("Escape");
      await dialog().waitForExist({ reverse: true, timeout: 15000 });
      await openArtifact(browser, "a:docs", "Artifact docs");
      await waitFor(browser, async () => JSON.stringify((await definition(browser))?.parts) === '["kit"]', "docs's Definition lists kit");

      // Rename: the file and every reference follow in one edit.
      await openArtifact(browser, "a:core", "Artifact core");
      const oldCoordinates = placeOf(fixture, "core", "at");
      const oldBox = await settled(browser, "a:core");
      assert.ok(oldCoordinates, "the renamed Artifact was placed by hand");
      await browser.$(DETAIL).$("button=Rename").click();
      await dialog().waitForExist({ timeout: 15000 });
      assert.equal(await dialog().$(".dialog-title").getText(), "Rename core");
      assert.equal(await dialog().$("input").getValue(), "core", "the field holds the name");
      // A name another Artifact has, or no name at all, is refused under the field.
      const named = ["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name));
      for (const [wrong, reason] of [["docs", /docs.*already exists/i], ["two words", /name.*(?:invalid|must|expected)|invalid.*name/i]] as const) {
        await fill(browser, '[role="dialog"] input', wrong);
        await dialog().$("button=Rename").click();
        await waitFor(browser, async () => await explains("input[aria-label=Name]", reason) && (await dialog().$("input").getAttribute("aria-invalid")) === "true" && (await dialog().$("input").getValue()) === wrong, `${wrong} refused under the field`);
        assert.deepEqual(["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name)), named, `${wrong}: nothing written`);
      }
      await fill(browser, '[role="dialog"] input', "base");
      await dialog().$("button=Rename").click();
      await waitFor(browser, () => file(fixture, "base") !== null && file(fixture, "core") === null
        && (file(fixture, "docs")?.includes("{base}") ?? false) && !file(fixture, "docs")?.includes("{core}"),
      "core.artf is renamed and docs now references base");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact base", "the Detail names base");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:base")}`).isExisting(), "the graph draws base");
      assert.deepEqual(placeOf(fixture, "base", "at"), oldCoordinates, "rename preserves its own coordinates");
      const renamedBox = await settled(browser, "a:base");
      assert.ok(Math.abs(renamedBox.left - oldBox.left) <= 1 && Math.abs(renamedBox.top - oldBox.top) <= 1, "the renamed node is drawn in the same place");
      // A part held by several wholes, placed in one of them and with a comment: every parts
      // entry and the place follow, its file stays in its folder with its comment.
      const kitText = file(fixture, "kit");
      const appPlace = placeOf(fixture, "app", "at");
      const kitPlace = placeOf(fixture, "app", "kit");
      assert.ok(appPlace, "the fixture placed app before renaming its part");
      assert.deepEqual(kitPlace, [64, 48], "kit starts away from its automatic position in app's box");
      await openArtifact(browser, "a:app/a:kit", "Artifact kit");
      await browser.$(DETAIL).$("button=Rename").click();
      await dialog().waitForExist({ timeout: 15000 });
      await fill(browser, '[role="dialog"] input', "piece");
      await browser.keys("Enter");
      const wholes = ["app", "bundle", "docs"];
      await waitFor(browser, () => file(fixture, "kit") === null
        && exists(nativePath(fixture.repo, ".artifactize", "lib", "piece.artf")) && file(fixture, "piece") === kitText
        && wholes.every((whole) => holds(fixture, whole, "piece") && !holds(fixture, whole, "kit"))
        && placesPart(fixture, "app", "piece") && !placesPart(fixture, "app", "kit"),
      "lib/kit.artf renamed lib/piece.artf, with all three references and its placement renamed");
      assert.equal(file(fixture, "piece"), kitText, "its text, comment included, as it was");
      for (const whole of wholes) {
        assert.ok(holds(fixture, whole, "piece") && !holds(fixture, whole, "kit"), `${whole}'s parts name piece`);
      }
      assert.deepEqual(placeOf(fixture, "app", "piece"), kitPlace, "app's exact saved place follows kit to piece");
      assert.equal(placeOf(fixture, "app", "kit"), null, "the old name's place is gone");
      assert.deepEqual(placeOf(fixture, "app", "at"), appPlace, "app keeps its own numeric coordinates");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact piece", "the Detail names piece");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app/a:piece")}`).isExisting(), "the graph draws piece in app's box");
      await waitFor(browser, async () => (await comments(browser))["a:app/a:piece"] === "Kit is shared.", "its comment drawn under it");
      // Every edit names the text it was shown: a file another writer changed meanwhile refuses
      // it, whether made in the Detail, a dialog or on the graph, writing nothing; the refusal
      // names the file, and the graph is drawn from what is on disk now.
      const all = () => ["base", "docs", "app", "bundle", "piece", "extra"].map((name) => file(fixture, name));
      const theirs = (name: string, text: string) => {
        writeText(pathOf(fixture, name), `${file(fixture, name)}\n[annotations.comment]\ntext = ${JSON.stringify(text)}\n`);
        return all();
      };
      const refusedFor = async (name: string, text: string, kept: (string | null)[], said: () => Promise<string>) => {
        await waitFor(browser, async () => (await said()) === `${relativeTextPath(fixture.repo, pathOf(fixture, name))} changed on disk.`, `${name}'s change refuses the edit, naming the file`);
        assert.deepEqual(all(), kept, `${name}: nothing written`);
        await waitFor(browser, async () => (await comments(browser))[`a:${name}`] === text, `the graph shows ${name}'s file as it is on disk`);
      };
      const inDetail = async () => ((await definition(browser))?.refused ?? []).join();
      // The Path.
      await openArtifact(browser, "a:base", "Artifact base");
      let kept = theirs("base", "theirs: path");
      await fill(browser, `${DETAIL} input[aria-label="Path of base"]`, "elsewhere/");
      await browser.keys("Enter");
      await refusedFor("base", "theirs: path", kept, inDetail);
      await browser.keys("Escape");
      // A part.
      await openArtifact(browser, "a:bundle", "Artifact bundle");
      kept = theirs("bundle", "theirs: parts");
      await browser.$(DETAIL).$("button=Add").click();
      await browser.$(`${DETAIL} input[aria-label="Add a part to bundle"]`).waitForExist({ timeout: 15000 });
      await browser.keys("docs");
      await browser.keys("Enter");
      await refusedFor("bundle", "theirs: parts", kept, inDetail);
      await browser.keys("Escape");
      // A later external edit wins against removing a part too.
      writeText(pathOf(fixture, "bundle"), (file(fixture, "bundle") ?? "").replace("theirs: parts", "theirs: keep the part"));
      kept = all();
      await browser.$(`${DETAIL} button[aria-label="Remove piece from the parts of bundle"]`).click();
      await refusedFor("bundle", "theirs: keep the part", kept, inDetail);
      await waitFor(browser, async () => (await definition(browser))?.parts.includes("piece") === true &&
        await browser.$(`${CURRENT} ${nodeAt("a:bundle/a:piece")}`).isExisting(), "the refreshed Definition and graph keep the part");
      await browser.keys("Escape");
      // Delete.
      await openArtifact(browser, "a:extra", "Artifact extra");
      kept = theirs("extra", "theirs: delete");
      await browser.$(DETAIL).$("button=Delete").click();
      await dialog().waitForExist({ timeout: 15000 });
      await dialog().$("button=Delete").click();
      await refusedFor("extra", "theirs: delete", kept, async () => ((await refusal().isExisting()) ? await refusal().getText() : ""));
      await dialog().$("button=Cancel").click();
      await dialog().waitForExist({ reverse: true, timeout: 15000 });
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed, the whole graph in reach");
      await browser.$(TOOLS).$("button=Fit").click();
      await graphShown(browser, "a:base");
      // A move on the graph.
      kept = theirs("docs", "theirs: move");
      const docsAt = (await box(browser, "a:docs"))!;
      await drag(browser, docsAt, await below(browser, docsAt.x));
      await refusedFor("docs", "theirs: move", kept, () => notice(browser));
      // A drop into a box.
      kept = theirs("app", "theirs: drop");
      const baseAt = (await box(browser, "a:base"))!;
      const appAt = (await box(browser, "a:app"))!;
      await drag(browser, baseAt, { x: appAt.left + 60, y: appAt.top + 14 });
      await refusedFor("app", "theirs: drop", kept, () => notice(browser));
      assert.equal(await browser.$(`${CURRENT} ${nodeAt("a:app/a:base")}`).isExisting(), false, "base not drawn in app's box");
      // Reset layout.
      writeText(pathOf(fixture, "bundle"), (file(fixture, "bundle") ?? "").replace("theirs: keep the part", "theirs: reset"));
      kept = all();
      await browser.$(TOOLS).$("button=Reset layout").click();
      await refusedFor("bundle", "theirs: reset", kept, () => notice(browser));
      assert.ok(["base", "docs", "app", "bundle"].every((name) => ownAt(fixture, name) !== null), "every place kept");
      await openArtifact(browser, "a:base", "Artifact base");

      // Delete: refused while docs names base, saying which; done for extra.
      await browser.$(DETAIL).$("button=Delete").click();
      await dialog().waitForExist({ timeout: 15000 });
      assert.equal(await dialog().$(".dialog-title").getText(), "Delete base?");
      assert.equal(await dialog().$("p").getText(), ".artifactize/base.artf is removed.");
      await dialog().$("button=Delete").click();
      await waitFor(browser, async () => (await refusal().isExisting()) && /docs/.test(await refusal().getText()), "the deletion refused, naming docs");
      assert.notEqual(file(fixture, "base"), null, "nothing removed");
      await dialog().$("button=Cancel").click();
      await dialog().waitForExist({ reverse: true, timeout: 15000 });
      // piece is named only in parts lists, not in an eval reference.
      await openArtifact(browser, "a:app/a:piece", "Artifact piece");
      const beforePartDelete = all();
      await browser.$(DETAIL).$("button=Delete").click();
      await dialog().waitForExist({ timeout: 15000 });
      assert.equal(await dialog().$(".dialog-title").getText(), "Delete piece?");
      await dialog().$("button=Delete").click();
      await waitFor(browser, async () => await refusal().isDisplayed() && /\bapp\b/.test(await refusal().getText()) && /\bbundle\b/.test(await refusal().getText()), "deletion refused in the same dialog, naming the wholes app and bundle");
      assert.deepEqual(all(), beforePartDelete, "the part and both whole definitions stayed unchanged");
      await dialog().$("button=Cancel").click();
      await dialog().waitForExist({ reverse: true, timeout: 15000 });
      await openArtifact(browser, "a:extra", "Artifact extra");
      await browser.$(DETAIL).$("button=Delete").click();
      await dialog().waitForExist({ timeout: 15000 });
      await dialog().$("button=Delete").click();
      await waitFor(browser, () => file(fixture, "extra") === null, "extra's file removed");
      await dialog().waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "the successful Delete closes its confirmation" });
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "its Detail closed");
      await waitFor(browser, async () => !(await browser.$(`${CURRENT} ${nodeAt("a:extra")}`).isExisting()), "its node gone");
    },
  ),

  // UX journeys: J-NODE-MOVE J-LAYOUT-RESET J-PARTS-DRAG J-PARTS-EDIT J-DEFINITION
  scenario<{ run: string }>(
    "editing-layout",
    "places Artifacts by hand and makes parts by dropping them, saving places in the definitions, which draw the branch's Run too and hold after a restart",
    async () => {
      const fixture = createEditing({ comments: true });
      const verified = await verifyAll(fixture);
      if (verified.code !== 0) throw new Error(`the editing fixture did not verify: ${verified.stdout}`);
      const { id: run } = JSON.parse(verified.stdout) as { id: string };
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { run } };
    },
    async (browser, { fixture, data: { run } }) => {
      await currentShown(browser, "a:core");
      assert.equal(await browser.$(TOOLS).$("button=Reset layout").isExisting(), false, "the top level is laid out automatically");
      await afterRender(browser);
      const initial = await places(browser);
      // A press and release without moving opens the Detail.
      const core = (await box(browser, "a:core"))!;
      await clickAt(browser, { x: Math.round(core.x), y: Math.round(core.y) });
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact core", "a click without movement opens the Detail");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed, the whole graph in reach");
      // Esc during a drag puts the node back, writing nothing.
      const files = () => ["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name));
      const untouched = files();
      await drag(browser, core, await below(browser, core.x + 80, core.y), {
        held: () => waitFor(browser, async () => {
          const moved = await box(browser, "a:core");
          return !!moved && Math.hypot(moved.x - core.x, moved.y - core.y) > 20;
        }, "core visibly moves before Escape"),
        escape: true,
        escaped: () => waitFor(browser, async () => JSON.stringify(await box(browser, "a:core")) === JSON.stringify(core), "Esc put core back before the button is let go"),
      });
      await afterRender(browser);
      assert.deepEqual(await box(browser, "a:core"), core, "core back where it was");
      assert.deepEqual(files(), untouched, "nothing written");
      // While dragged, core is raised and follows the pointer; its edge and its comment come along.
      // Released, it is placed: the first move writes every top-level node's place, each in its
      // own file.
      const moved = await below(browser, core.x + 40);
      const [edgeBefore, commentBefore, cameraFirst] = [await edgeOf(browser, "a:core"), (await commentOf(browser, "a:core"))!, await camera(browser)];
      await drag(browser, { x: core.x, y: core.y }, moved, {
        held: async () => {
          assert.equal(await lifted(browser, "a:core"), true, "core raised while dragged");
          const now = (await box(browser, "a:core"))!;
          // Within a frame's lag of the pointer, far from where it was.
          assert.ok(Math.hypot(now.x - moved.x, now.y - moved.y) < 40 && Math.hypot(now.x - core.x, now.y - core.y) > Math.hypot(moved.x - core.x, moved.y - core.y) - 40, `core follows the pointer: ${JSON.stringify(now)} for ${JSON.stringify(moved)}`);
          assert.notEqual(await edgeOf(browser, "a:core"), edgeBefore, "its edge follows");
          const note = (await commentOf(browser, "a:core"))!;
          assert.ok(Math.abs(note.box.top - note.node.bottom - (commentBefore.box.top - commentBefore.node.bottom)) < 1 && note.box.top !== commentBefore.box.top, "its comment comes along under it");
          assert.deepEqual(files(), untouched, "nothing written before the release");
        },
      });
      await waitFor(browser, () => ["core", "docs", "app", "bundle"].every((name) => /\bat = \[/.test(file(fixture, name) ?? "")), "every top-level node's place written");
      assert.equal(/annotations/.test(file(fixture, "kit") ?? ""), false, "a part has no top-level place");
      await waitFor(browser, async () => (await browser.$(TOOLS).$("button=Reset layout").isExisting()), "Reset layout offered for the hand-placed top level");
      assert.equal(await camera(browser), cameraFirst, "the camera stays as the top level is placed");
      const placed = await box(browser, "a:core");
      // The node follows the pointer from where the drag began, a few pixels past the press.
      assert.ok(placed && Math.abs(placed.x - moved.x) < 12 && Math.abs(placed.y - moved.y) < 12, `core stays where it was dropped: ${JSON.stringify(placed)} for ${JSON.stringify(moved)}`);
      // A later move writes the moved node's file only.
      const docsBefore = file(fixture, "docs");
      const coreAt = ownAt(fixture, "core");
      // Find opens the Detail and puts the drag source in the uncovered part of the graph.
      await browser.$(TOOLS).$("button=Find").click();
      await fill(browser, FIND, "core");
      await waitFor(browser, async () => (await findField(browser))?.entries.some((entry) => entry.name === "core") === true, "Find lists core");
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === "Artifact core", "Find opens core's Detail");
      const beforeSelected = await browser.$(`${CURRENT} ${nodeAt("a:core")} .artifact`).getAttribute("class");
      assert.match(beforeSelected ?? "", /\bopen\b/, "the moved node is selected with its Detail open");
      const cameraWithDetail = await camera(browser);
      await graphShown(browser, "a:core");
      const selected = (await box(browser, "a:core"))!;
      const next = await below(browser, selected.x + 40, selected.y);
      await drag(browser, selected, next);
      await waitFor(browser, () => ownAt(fixture, "core") !== coreAt, "core's new place written");
      await waitFor(browser, async () => {
        const at = await box(browser, "a:core");
        return !!at && Math.hypot(at.x - next.x, at.y - next.y) < 12;
      }, "core moved again");
      assert.equal(file(fixture, "docs"), docsBefore, "docs's file untouched");
      assert.equal((await detail(browser))?.title, "Artifact core", "placing keeps the active Detail");
      assert.match(await browser.$(`${CURRENT} ${nodeAt("a:core")} .artifact`).getAttribute("class") ?? "", /\bopen\b/, "placing keeps the same selection");
      assert.equal(await camera(browser), cameraWithDetail, "placing with an open Detail preserves the camera");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => (await detail(browser)) === null, "the Detail closed before the next drop");
      await browser.$(TOOLS).$("button=Fit").click();
      await camera(browser);

      // A whole is dragged by its title row, carrying its parts; minimized, it is dragged as a node.
      const bundleTitle = await titleOf(browser, "a:bundle");
      const kitHeld = (await box(browser, "a:bundle/a:kit"))!;
      const bundleHeld = (await box(browser, "a:bundle"))!;
      const bundleAt = ownAt(fixture, "bundle");
      const bundleTo = await below(browser, bundleTitle.x);
      await drag(browser, bundleTitle, bundleTo, {
        held: async () => {
          assert.equal(await lifted(browser, "a:bundle"), true, "bundle raised while dragged");
          const [kitNow, bundleNow] = [(await box(browser, "a:bundle/a:kit"))!, (await box(browser, "a:bundle"))!];
          assert.ok(Math.hypot(bundleNow.x - bundleHeld.x, bundleNow.y - bundleHeld.y) > 40, "bundle follows the pointer");
          const [dx, dy] = [kitNow.x - kitHeld.x - (bundleNow.x - bundleHeld.x), kitNow.y - kitHeld.y - (bundleNow.y - bundleHeld.y)];
          assert.ok(Math.hypot(dx, dy) < 2, `its part kit carried along with it: off by ${dx}, ${dy}`);
        },
      });
      await waitFor(browser, () => ownAt(fixture, "bundle") !== bundleAt, "bundle's new place written");
      await browser.$(`${CURRENT} ${nodeAt("a:bundle")} button[aria-label^="Minimize bundle"]`).click();
      await waitFor(browser, async () => !(await browser.$(`${CURRENT} ${nodeAt("a:bundle/a:kit")}`).isExisting()), "bundle minimized");
      // The collapsed bundle keeps its previous top-left. Fit first if that move
      // left its center behind an edge of this smaller viewport.
      await browser.$(TOOLS).$("button=Fit").click();
      await graphShown(browser, "a:bundle");
      await camera(browser);
      const folded = await visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:bundle")} .whole`).getElement());
      const foldedAt = ownAt(fixture, "bundle");
      await drag(browser, folded, await below(browser, folded.x + 30, folded.y));
      await waitFor(browser, () => ownAt(fixture, "bundle") !== foldedAt, "the minimized bundle's place written");
      await browser.$(`${CURRENT} ${nodeAt("a:bundle")} button[aria-label^="Expand bundle"]`).click();
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:bundle/a:kit")}`).isExisting(), "bundle expanded");
      // Fit brings the moved bundle back into view.
      await browser.$(TOOLS).$("button=Fit").click();
      await graphShown(browser, "a:bundle/a:kit");

      // Dropped into app's box, core becomes app's part; app's places are written as shown. The
      // box is highlighted while the drop would be accepted.
      const appTitle = await titleOf(browser, "a:app");
      const into = (await box(browser, "a:app"))!;
      const at = (await box(browser, "a:core"))!;
      const cameraJoin = await camera(browser);
      // Onto app's title row: inside its box, over none of its parts.
      await drag(browser, at, { x: into.left + 60, y: into.top + 14 }, {
        held: async () => waitFor(browser, () => targeted(browser, "a:app"), "app's box highlighted as the drop target"),
      });
      await waitFor(browser, () => /parts = \[[^\]]*"core"/.test(file(fixture, "app") ?? ""), "app's file names core as a part");
      await waitFor(browser, () => /\bcore = \[/.test(file(fixture, "app") ?? "") && /\bkit = \[/.test(file(fixture, "app") ?? ""), "core's place in app's box written, and kit's as shown");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app/a:core")}`).isExisting(), "core drawn in app's box");
      assert.equal(await camera(browser), cameraJoin, "the camera stays as core joins app");
      // Dropped over app's title row it would cover kit: it goes under app's parts, and is drawn
      // where its place says.
      const [corePlace, kitPlace] = [placeOf(fixture, "app", "core")!, placeOf(fixture, "app", "kit")!];
      assert.ok(corePlace[1] >= kitPlace[1] + 44, `core placed under kit: ${JSON.stringify([corePlace, kitPlace])}`);
      await waitFor(browser, async () => {
        const shown = await places(browser);
        const [inBox, kitAt] = [shown["a:app/a:core"], shown["a:app/a:kit"]];
        return !!inBox && !!kitAt && Math.abs(inBox[0] - kitAt[0] - (corePlace[0] - kitPlace[0])) <= 1 && Math.abs(inBox[1] - kitAt[1] - (corePlace[1] - kitPlace[1])) <= 1;
      }, "core drawn in app's box where its place says");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label="Reset layout of app"]`).isExisting(), "the hand-placed box offers Reset layout");

      // A part added from the Detail to the hand-placed box goes where it covers none of its
      // parts.
      await openArtifact(browser, "a:app", "Artifact app");
      await browser.$(DETAIL).$("button=Add").click();
      await browser.$(`${DETAIL} input[aria-label="Add a part to app"]`).waitForExist({ timeout: 15000 });
      await browser.keys("docs");
      await browser.keys("Enter");
      await waitFor(browser, () => holds(fixture, "app", "docs"), "app's file names docs as a part");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app/a:docs")}`).isExisting(), "the graph draws docs in app's box");
      const covered = await browser.execute((root) => {
        const rects = ["a:app/a:docs", "a:app/a:kit", "a:app/a:core"].map((id) => document.querySelector(`${root} .svelte-flow__node[data-id="${id}"]`)!.getBoundingClientRect());
        const [added, ...others] = rects;
        return others.some((at) => at.left < added.right && added.left < at.right && at.top < added.bottom && added.top < at.bottom);
      }, CURRENT);
      assert.equal(covered, false, "docs covers none of app's parts");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed");
      // Moved inside app's box, docs's place is written in app's file alone, nothing before the
      // release; the camera stays.
      // Adding a part may grow the whole past the current camera. Bring the
      // complete box into view before testing a place in its lower margin.
      await browser.$(TOOLS).$("button=Fit").click();
      await camera(browser);
      const inBox = await settled(browser, "a:app/a:docs");
      const appBox = await settled(browser, "a:app");
      const others = () => ["core", "docs", "kit", "bundle"].map((name) => file(fixture, name));
      const [othersBefore, appBefore0, cameraBefore] = [others(), file(fixture, "app"), await camera(browser)];
      // Drop in the lower margin of the whole, below the nodes. This is a place within
      // the box (not a drop on another part), and avoids clamping against its inner top.
      const dropIn = { x: appBox.left + appBox.width - 5, y: appBox.top + appBox.height - 5 };
      const siblings = ["kit", "core", "docs"];
      const beforePlaces = await places(browser);
      await drag(browser, inBox, dropIn, {
        held: async () => assert.equal(file(fixture, "app"), appBefore0, "nothing written while held"),
      });
      await waitFor(browser, () => file(fixture, "app") !== appBefore0 && siblings.every((name) => placesPart(fixture, "app", name)),
        "app's file records the moved part and every displayed sibling");
      await waitFor(browser, async () => {
        const at = await box(browser, "a:app/a:docs");
        if (!at || Math.hypot(at.x - dropIn.x, at.y - dropIn.y) > 2) return false;
        const shown = await places(browser);
        const written = placeOf(fixture, "app", "docs")!;
        const moved = shown["a:app/a:docs"];
        return siblings.filter((name) => name !== "docs").every((name) => {
          const position = shown[`a:app/a:${name}`], saved = placeOf(fixture, "app", name)!;
          const before = beforePlaces[`a:app/a:${name}`];
          return !!position && position.every((axis, i) => Math.abs(axis - moved[i] - (saved[i] - written[i])) <= 1
            && Math.abs(axis - before[i]) <= 1);
        });
      }, "the moved part stays at release and every sibling's saved offset matches the display");
      assert.deepEqual(others(), othersBefore, "no other file written");
      assert.equal(await camera(browser), cameraBefore, "the camera stays");
      // Removed from the Detail, docs leaves app's parts and its place there.
      await openArtifact(browser, "a:app", "Artifact app");
      await browser.$(`${DETAIL} button[aria-label="Remove docs from the parts of app"]`).click();
      await waitFor(browser, () => !holds(fixture, "app", "docs") && !placesPart(fixture, "app", "docs"), "docs gone from app's parts and places");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed");
      await graphShown(browser, "a:docs");

      // Dropped out of every box, it leaves app; its own place is written.
      const part = (await box(browser, "a:app/a:core"))!;
      const app = (await box(browser, "a:app"))!;
      const released = await below(browser, app.left + app.width + 60);
      await drag(browser, part, released);
      await waitFor(browser, () => !holds(fixture, "app", "core"), "core gone from app's parts");
      assert.equal(placesPart(fixture, "app", "core"), false, "and from app's places");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:core")}`).isExisting(), "core drawn at the top level");
      assert.notEqual(ownAt(fixture, "core"), null, "core's own place written");
      // Drawn where it was let go, and where its written place says against docs's.
      // Against bundle's, which keeps its own place.
      const [coreOwn, bundleOwn] = [placeOf(fixture, "core", "at")!, placeOf(fixture, "bundle", "at")!];
      await waitFor(browser, async () => {
        const [shown, left] = [await places(browser), (await box(browser, "a:core"))!];
        const [core, bundle] = [shown["a:core"], shown["a:bundle"]];
        return !!core && !!bundle && Math.abs(core[0] - bundle[0] - (coreOwn[0] - bundleOwn[0])) <= 1 && Math.abs(core[1] - bundle[1] - (coreOwn[1] - bundleOwn[1])) <= 1
          && Math.hypot(left.x - released.x, left.y - released.y) < 40;
      }, "core drawn where it was let go and where its place says");
      // A drop that would make a cycle: app onto the copy of its own part kit in bundle.
      const kitInBundle = (await box(browser, "a:bundle/a:kit"))!;
      const definitionsBefore = [file(fixture, "app"), ...others()];
      const where = await titleOf(browser, "a:app");
      const appShape = await box(browser, "a:app");
      await drag(browser, where, { x: kitInBundle.x, y: kitInBundle.y }, {
        held: async () => assert.equal(await targeted(browser, "a:bundle/a:kit"), false, "a cycle is no drop target"),
      });
      await waitFor(browser, async () => /\b(?:app|kit) is a part of itself through its parts\b/.test(await notice(browser)), "the notice explains the circular parts relationship");
      await waitFor(browser, async () => JSON.stringify(await box(browser, "a:app")) === JSON.stringify(appShape), "app back where it was, as it was");
      assert.deepEqual([file(fixture, "app"), ...others()], definitionsBefore, "every definition is untouched by the refused drop");
      assert.ok(appTitle, "app's title row is where a whole is dragged from");

      // A shared part dragged from app onto docs leaves app alone: bundle's copy stays where it is.
      const kitInApp = (await box(browser, "a:app/a:kit"))!;
      const docsNode = (await box(browser, "a:docs"))!;
      const bundleKit = (await places(browser))["a:bundle/a:kit"];
      await drag(browser, kitInApp, { x: docsNode.x, y: docsNode.y }, { held: async () => waitFor(browser, () => targeted(browser, "a:docs"), "docs highlighted as the drop target") });
      await waitFor(browser, () => holds(fixture, "docs", "kit") && !holds(fixture, "app", "kit"), "kit moved from app's parts to docs's");
      assert.equal(placesPart(fixture, "app", "kit"), false, "kit's place gone from app's file");
      assert.ok(holds(fixture, "bundle", "kit"), "bundle still holds kit");
      const arrived = await settled(browser, "a:docs/a:kit");
      assert.deepEqual((await places(browser))["a:bundle/a:kit"], bundleKit, "bundle's copy of kit stays where it was");
      const written = placeOf(fixture, "docs", "kit");
      assert.ok(written?.every(Number.isFinite), "docs owns the dropped part's new coordinates");
      const destination = await settled(browser, "a:docs");
      assert.ok(arrived.left >= destination.left && arrived.top > destination.top && arrived.left < destination.left + destination.width, "the dropped part is drawn inside its destination");
      assert.ok(Math.abs(arrived.x - docsNode.x) <= arrived.width && Math.abs(arrived.y - docsNode.y) <= arrived.height * 2,
        `the part is drawn at the drop on docs, not an unrelated position: ${JSON.stringify({ arrived, docsNode, written })}`);
      // The drop may have grown docs past the canvas edge. Fit after checking the
      // placement in the original camera, so both ends of the return drag are reachable.
      await browser.$(TOOLS).$("button=Fit").click();
      await camera(browser);
      await graphShown(browser, "a:docs/a:kit");
      const kitInDocs = (await box(browser, "a:docs/a:kit"))!;
      const appNode = (await box(browser, "a:app"))!;
      await drag(browser, kitInDocs, { x: appNode.x, y: appNode.y });
      await waitFor(browser, () => holds(fixture, "app", "kit") && !holds(fixture, "docs", "kit"), "kit back in app");
      await graphShown(browser, "a:app/a:kit");

      // Reset layout of the top level keeps the boxes placed by hand and the camera; Reset layout
      // of app's box then; the graph is laid out as it first was.
      const zoomed = await camera(browser);
      await browser.$(TOOLS).$("button=Reset layout").click();
      await waitFor(browser, () => ["core", "docs", "app", "bundle"].every((name) => ownAt(fixture, name) === null), "every top-level place gone");
      await waitFor(browser, async () => !(await browser.$(TOOLS).$("button=Reset layout").isExisting()), "Reset layout gone with the places");
      assert.ok(/annotations\.style\.parts/.test(file(fixture, "app") ?? ""), "app's places kept");
      assert.equal(await camera(browser), zoomed, "the camera stays");
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label="Reset layout of app"]`).click();
      await waitFor(browser, () => !/annotations\.style\.parts/.test(file(fixture, "app") ?? ""), "app's places gone");
      let reset = {};
      await waitFor(browser, async () => JSON.stringify(reset = await places(browser)) === JSON.stringify(initial), "laid out as it first was")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(reset)} after reset, ${JSON.stringify(initial)} initially`); });

      // Placed again, the places draw the branch's Run as they draw its Current.
      const again = (await box(browser, "a:docs"))!;
      await drag(browser, again, await below(browser, again.x + 50));
      await waitFor(browser, () => /\bat = \[/.test(file(fixture, "docs") ?? ""), "docs placed");
      await afterRender(browser);
      const onCurrent = await places(browser);
      writeText(nativePath(fixture.root, "places.json"), JSON.stringify(onCurrent));
      await showRun(browser, "repo  master", run);
      await graphShown(browser, "a:docs");
      let onRunPlaces = {};
      await waitFor(browser, async () => JSON.stringify(onRunPlaces = await places(browser)) === JSON.stringify(onCurrent), "the Run's graph drawn with the same places")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(onRunPlaces)} on the Run, ${JSON.stringify(onCurrent)} on the Current`); });
      // A Run's graph is a record: nothing moves, nothing is offered to edit.
      const onRun = (await box(browser, "a:docs"))!;
      const runCamera = await camera(browser);
      // Docs was placed near the bottom; pan it up into the graph, not out of the window.
      await drag(browser, onRun, { x: onRun.x, y: onRun.y - 80 });
      await waitFor(browser, async () => (await camera(browser)) !== runCamera, "the drag on a Run panned the camera");
      assert.equal(await browser.$(TOOLS).$("button=New Artifact").isExisting(), false, "no New Artifact on a Run");
      assert.equal(await browser.$(TOOLS).$("button=Reset layout").isExisting(), false, "no Reset layout on a Run");
      const after = await places(browser);
      assert.deepEqual(after, onCurrent, "the drag on a Run panned the camera, moving no node against another");
      await browser.$(nodeAt("a:docs")).click();
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact docs", "a Run's Artifact opens its Detail");
      await waitFor(browser, async () => await browser.$(`${DETAIL} section[aria-label="Path"]`).isExisting(), "a Run's Detail shows its Path");
      assert.equal(await browser.$(`${DETAIL} section[aria-label="Definition"]`).isExisting(), false, "and no Definition");
    },
    async (browser, { fixture }) => {
      // After a restart the places still hold: the graph is drawn where it was left.
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master" && /^Scanned at \d/.test(await scanned(browser)), "the branch's Current was scanned after restart");
      await waitFor(browser, async () => await browser.$(TOOLS).$("button=Reset layout").isExisting(), "the top level still placed by hand");
      await browser.$(TOOLS).$("button=Fit").click();
      await camera(browser);
      await graphShown(browser, "a:docs");
      const saved = readText(nativePath(fixture.root, "places.json"));
      let shown = {};
      await waitFor(browser, async () => JSON.stringify(shown = await places(browser)) === saved, "every Artifact where it was left")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(shown)}, saved ${saved}`); });
    },
  ),

  // UX journeys: J-NODE-MOVE J-LAYOUT-RESET
  scenario<null>(
    "editing-nested-layout",
    "pans from the face of a whole and resets an outer box without resetting its hand-placed inner box",
    async () => {
      const fixture = createEditing({ nested: true, placed: true });
      writeText(pathOf(fixture, "bundle"), `path = "bundle/"
parts = ["app", "docs"]
basis = true
[annotations.style]
at = [420.0, 0.0]
[annotations.style.parts]
app = [248.0, 96.0]
docs = [0.0, 0.0]
`);
      writeText(pathOf(fixture, "app"), file(fixture, "app")!.replace("kit = [0.0, 0.0]", "kit = [48.0, 32.0]"));
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    async (browser, { fixture }) => {
      const outer = "a:bundle", inner = `${outer}/a:app`, kit = `${inner}/a:kit`;
      const reset = (id: string, name: string) => browser.$(`${CURRENT} ${nodeAt(id)} button[aria-label="Reset layout of ${name}"]`);
      await currentShown(browser, kit);
      await reset(outer, "bundle").waitForDisplayed({ timeout: 15000 });
      await reset(inner, "app").waitForDisplayed({ timeout: 15000 });
      const definitions = () => ["bundle", "app", "kit", "docs", "core"].map((name) => file(fixture, name));
      const before = definitions();
      const beforePlaces = await places(browser);
      const cameraBefore = await camera(browser);
      const face = await browser.execute((selector) => {
        const surface = document.querySelector(`${selector} > .whole`);
        const title = surface?.querySelector(".title");
        const canvas = surface?.closest(".canvas");
        if (!surface || !title || !canvas) return null;
        const box = surface.getBoundingClientRect(), room = canvas.getBoundingClientRect();
        for (let y = Math.ceil(Math.max(title.getBoundingClientRect().bottom, room.top)) + 4; y < Math.min(box.bottom, room.bottom) - 4; y += 4) {
          for (let x = Math.ceil(Math.max(box.left, room.left)) + 4; x < Math.min(box.right, room.right) - 4; x += 4) {
            if (document.elementFromPoint(x, y) === surface) {
              return { from: { x, y }, to: { x: x + (x + 70 < room.right ? 60 : -60), y } };
            }
          }
        }
        return null;
      }, `${CURRENT} ${nodeAt(outer)}`);
      assert.ok(face, "a reachable point on bundle's face, below its title and outside its parts");
      await drag(browser, face.from, face.to);
      await waitFor(browser, async () => (await camera(browser)) !== cameraBefore, "dragging the face pans the camera");
      await afterRender(browser);
      assert.deepEqual(await places(browser), beforePlaces, "panning moves none of the Artifacts relative to each other");
      assert.deepEqual(definitions(), before, "the face drag writes no placement");

      const relative = async (child: string, parent: string) => {
        const [one, other] = await Promise.all([box(browser, child), box(browser, parent)]);
        return one && other ? [one.left - other.left, one.top - other.top].map(Math.round) : null;
      };
      const outerOffset = await relative(inner, outer);
      const innerOffset = await relative(kit, inner);
      assert.ok(outerOffset && innerOffset, "both hand-placed levels are shown");
      assert.deepEqual(placeOf(fixture, "app", "kit"), [48, 32], "the inner box really has a saved place");
      await reset(outer, "bundle").click();
      await waitFor(browser, () => placeOf(fixture, "bundle", "app") === null && placeOf(fixture, "bundle", "docs") === null,
        "only the outer box's saved part positions were removed");
      await waitFor(browser, async () => {
        const laidOut = await relative(inner, outer);
        return laidOut !== null && JSON.stringify(laidOut) !== JSON.stringify(outerOffset) && !(await reset(outer, "bundle").isExisting());
      }, "the outer box returns to automatic layout");
      assert.equal(await reset(inner, "app").isDisplayed(), true, "the inner box is still hand-placed");
      assert.deepEqual(await relative(kit, inner), innerOffset, "the inner box keeps its drawn part placement");
      assert.equal(file(fixture, "app"), before[1], "the inner box's definition and layout are unchanged");
    },
  ),

  // UX journey: J-PARTS-DRAG
  scenario<null>(
    "editing-shared-drop-position",
    "moves one copy of a shared part into an already expanded whole and draws its saved non-overlapping drop position",
    async () => {
      const fixture = createEditing({ nested: true, placed: true });
      const docs = pathOf(fixture, "docs");
      writeText(docs, `parts = ["core"]\n${file(fixture, "docs")}\n[annotations.style.parts]\ncore = [0.0, 0.0]\n`);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    async (browser, { fixture }) => {
      await currentShown(browser, "a:app/a:kit");
      const source = await settled(browser, "a:app/a:kit");
      const resident = await settled(browser, "a:docs/a:core");
      const whole = await settled(browser, "a:docs");
      const other = await settled(browser, "a:bundle/a:kit");
      const otherFile = file(fixture, "bundle");
      // The pointer is on the box's bare right margin, but the lifted card would cover core.
      const at = { x: (resident.left + resident.width + whole.left + whole.width) / 2, y: resident.y };
      await drag(browser, source, at, { held: () => waitFor(browser, () => targeted(browser, "a:docs"), "the existing whole is the drop target") });
      await waitFor(browser, () => holds(fixture, "docs", "kit") && !holds(fixture, "app", "kit"), "the part moved from app to docs");
      assert.equal(placesPart(fixture, "app", "kit"), false, "app's coordinates for kit are gone");
      assert.equal(file(fixture, "bundle"), otherFile, "the other whole's definition is untouched");
      const saved = placeOf(fixture, "docs", "kit");
      assert.ok(saved && saved[0] > 0 && saved[1] > 0, "the destination file has a new non-default position");
      const drawn = await settled(browser, "a:docs/a:kit");
      assert.deepEqual(await settled(browser, "a:bundle/a:kit"), other, "the other copy also stays in place on screen");
      const core = await settled(browser, "a:docs/a:core");
      const zoom = await browser.execute(() => {
        const node = document.querySelector<HTMLElement>('section[aria-label="Current"] .svelte-flow__node[data-id="a:docs/a:core"]')!;
        return node.getBoundingClientRect().width / node.offsetWidth;
      });
      assert.ok(drawn.top > core.top + core.height, "the overlap puts the part below the resident");
      assert.ok(Math.abs(drawn.left - core.left - saved[0] * zoom) <= 2 && Math.abs(drawn.top - core.top - saved[1] * zoom) <= 2,
        "the measured drawing uses exactly the destination file's relative coordinates");
    },
  ),

  // UX journeys: J-COMMENT J-COMMENT-EDIT J-COMMENT-MOVE
  scenario<{ run: string }>(
    "editing-comments",
    "draws each comment under its node, writes one from the Definition's Comment or a click on it, and moves one by dragging",
    async () => {
      const fixture = createEditing({ comments: true });
      const verified = await verifyAll(fixture);
      if (verified.code !== 0) throw new Error(`the editing fixture did not verify: ${verified.stdout}`);
      const { id: run } = JSON.parse(verified.stdout) as { id: string };
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { run } };
    },
    async (browser, { fixture, data: { run } }) => {
      await currentShown(browser, "a:core");
      await waitFor(browser, async () => JSON.stringify(await comments(browser)) === JSON.stringify({ "a:app": APP_NOTE, "a:core": CORE_NOTE }),
        `each comment under its node or its whole's title row: ${JSON.stringify(await comments(browser))}`);
      // Text alone in the quieter ink, within its node's width or its box, three lines at most.
      const quiet = async (id: string, inset: boolean) => {
        const note = (await commentOf(browser, id))!;
        assert.ok(note.quiet && note.bare, `${id}'s comment is text alone in the quieter ink`);
        assert.ok(note.box.left >= note.node.left - 1 && note.box.right <= note.node.right + 1, `${id}'s comment within its width`);
        if (inset) assert.ok(note.box.top >= note.node.top && note.box.bottom <= note.node.bottom, `${id}'s comment inside its box`);
        else assert.ok(note.box.top >= note.node.bottom, `${id}'s comment under its node`);
        assert.ok(note.box.height <= 3 * note.lineHeight + 1, `${id}'s comment shows three lines at most`);
        return note;
      };
      await quiet("a:app", true);
      const clamped = await quiet("a:core", false);
      assert.equal(clamped.cut, true, "core's fourth line overflows its visible box");
      // The fixture's third line stops with a plain letter, well before the
      // right edge. An ellipsis adds three visible dots after it; a plain clip
      // leaves that part of the line blank. The fourth line has no painted ink.
      const commentInk = async () => {
        const sample = await browser.execute((selector, lines) => {
          const comment = document.querySelector<HTMLElement>(selector)!;
          const box = comment.getBoundingClientRect();
          const style = getComputedStyle(comment);
          const context = document.createElement("canvas").getContext("2d")!;
          context.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
          // Measure the unclipped text, not a Range that may include the engine's ellipsis.
          const scale = box.width / comment.offsetWidth;
          const lineHeight = Number.parseFloat(style.lineHeight) * scale;
          const rows = lines.map((line, i) => ({ left: box.left, right: box.left + context.measureText(line).width * scale,
            top: box.top + i * lineHeight, bottom: box.top + (i + 1) * lineHeight }));
          const third = rows[2];
          return { rows, suffix: { ...third, left: third.right, right: box.right },
            viewport: { width: innerWidth, height: innerHeight },
            ink: (style.color.match(/\d+/g) ?? []).slice(0, 3).map(Number) };
        }, `${CURRENT} ${nodeAt("a:core")} .comment`, CORE_LINES);
        const frame = await screenshotPixels(browser, sample.viewport);
        const suffix = frame.inkColumns(sample.suffix, sample.ink);
        return { lines: sample.rows.map((row) => frame.inkColumns(row, sample.ink).some(Boolean)),
          dots: suffix.filter((on, index) => on && !suffix[index - 1]).length };
      };
      let paint: Awaited<ReturnType<typeof commentInk>> | undefined;
      await waitFor(browser, async () => {
        paint = await commentInk();
        return paint.lines.join() === "true,true,true,false" && paint.dots === 3;
      }, "three painted lines ending in an ellipsis, with the fourth line hidden")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(paint)}`); });
      // Read aloud right after its node's name or its whole's title row: the last text its node
      // holds, not hidden.
      const readAfter = (id: string) => browser.execute((selector) => {
        const node = document.querySelector(selector);
        const name = node?.querySelector(".name");
        const comment = node?.querySelector(".comment");
        if (!node || !name || !comment || comment.closest('[aria-hidden="true"]')) return false;
        const texts = [...node.querySelectorAll("*")].filter((each) => each.children.length === 0 && each.textContent?.trim() && !each.closest('[aria-hidden="true"]'));
        return (name.compareDocumentPosition(comment) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 && texts.at(-1) === comment;
      }, `${CURRENT} ${nodeAt(id)}`);
      for (const id of ["a:app", "a:core"]) assert.equal(await readAfter(id), true, `${id}'s comment read right after it`);
      // Minimized, a whole keeps its comment under it, and its parts' are not drawn.
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Minimize app"]`).click();
      await waitFor(browser, async () => {
        const minimized = await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Expand app"]`).isDisplayed();
        const note = await commentOf(browser, "a:app");
        return minimized && (await comments(browser))["a:app"] === APP_NOTE && !!note && note.box.top >= note.node.bottom;
      }, "app is visibly minimized with its comment below the collapsed node");
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Expand app"]`).click();
      await graphShown(browser, "a:app/a:kit");

      // The Definition's Comment opens the field under kit; Shift+Enter breaks the line, Enter
      // writes it, focus goes back to Comment.
      const KIT_NOTE = "Check the kit\nthen the bundle";
      const kitText = file(fixture, "kit");
      await openArtifact(browser, "a:app/a:kit", "Artifact kit");
      const commentButton = browser.$(`${DETAIL} button[data-comment-button]`);
      await browser.$(`${DETAIL} input[aria-label="Path of kit"]`).click();
      for (let step = 0; step < 12 && !(await commentButton.isFocused()); step += 1) await tab(browser);
      assert.equal(await commentButton.isFocused(), true, "Tab reaches Comment from the Definition");
      await buttonKey(browser, "Enter");
      await waitFor(browser, async () => {
        const editor = await field(browser);
        return editor?.focused === true && editor.under === "a:app/a:kit";
      }, "keyboard activation opens a focused editor directly below the chosen copy of kit");
      await browser.keys("Check the kit");
      // Shift+Enter breaks the line where the caret is, writing nothing.
      await press(browser, ["Shift", "Enter"], async () => (await field(browser))?.text === "Check the kit\n", "Shift+Enter breaks the line");
      assert.equal(file(fixture, "kit"), kitText, "Shift+Enter writes nothing");
      await browser.keys("then the bundle");
      let typedNote = "";
      await waitFor(browser, async () => (typedNote = (await field(browser))?.text ?? "") === KIT_NOTE, "the field holds two lines")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(typedNote)}`); });
      await browser.keys("Enter");
      await waitFor(browser, () => commentIn(fixture, "kit") === KIT_NOTE, "kit's comment written in its file, its line break kept");
      assert.equal(file(fixture, "kit")?.startsWith(kitText!), true, "nothing else in kit's file changed");
      await waitFor(browser, async () => (await comments(browser))["a:app/a:kit"] === KIT_NOTE && (await comments(browser))["a:bundle/a:kit"] === KIT_NOTE, "drawn under every copy of kit");
      for (const id of ["a:app/a:kit", "a:bundle/a:kit"]) assert.equal(await readAfter(id), true, `${id}'s comment read right after it`);
      await waitFor(browser, async () => await browser.$(`${DETAIL} button[data-comment-button]`).isFocused(), "focus back on Comment");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed, the whole graph in reach");
      // Minimized, app draws no comment of its parts.
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Minimize app"]`).click();
      await waitFor(browser, async () => !("a:app/a:kit" in (await comments(browser))) && (await comments(browser))["a:bundle/a:kit"] === KIT_NOTE, "a minimized whole's parts' comments not drawn");
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Expand app"]`).click();
      await graphShown(browser, "a:app/a:kit");

      // A click on core's comment opens it with its whole text; Esc puts it back, and so does a
      // click outside; an empty field removes it, focus back on core's node.
      const comment = () => browser.$(`${CURRENT} ${nodeAt("a:core")} .comment`);
      const coreText = file(fixture, "core");
      await pointerGapClick(browser, await visibleCenter(browser, await comment().getElement()));
      await waitFor(browser, async () => {
        const editor = await field(browser);
        return editor?.text === CORE_NOTE && editor.focused && editor.under === "a:core";
      }, "clicking core's comment opens its whole text in a focused editor below the node");
      const changeDraft = async () => {
        const draft = `${CORE_NOTE} now`;
        await fill(browser, 'textarea[aria-label="Comment on core"]', draft);
        await waitFor(browser, async () => (await field(browser))?.text === draft, "the changed draft is visible before cancellation");
      };
      const restored = async () => (await field(browser)) === null && await comment().isDisplayed()
        && (await comments(browser))["a:core"] === CORE_NOTE;
      await changeDraft();
      await browser.keys("Escape");
      await waitFor(browser, restored, "Esc closes the field and restores the old comment");
      assert.equal(file(fixture, "core"), coreText, "nothing written");
      await waitFor(browser, async () => (await comments(browser))["a:core"] === CORE_NOTE &&
        await browser.$(`${CURRENT} ${nodeAt("a:core")}`).isFocused(), "Esc restores the visible comment and focus to its graph node");
      await openArtifact(browser, "a:core", "Artifact core");
      await browser.$(DETAIL).$("button=Comment").click();
      await waitFor(browser, async () => (await field(browser))?.of === "core", "core's field open again");
      await changeDraft();
      // A click on the tool strip's Fit, outside the field.
      await browser.$(TOOLS).$("button=Fit").click();
      await waitFor(browser, restored, "an outside click closes the field and restores the old comment");
      assert.equal(file(fixture, "core"), coreText, "nothing written by a click outside");
      await waitFor(browser, async () => (await comments(browser))["a:core"] === CORE_NOTE &&
        await browser.$(`${DETAIL} button[data-comment-button]`).isFocused(), "clicking outside restores the visible comment and the Definition Comment button's focus");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => (await detail(browser)) === null, "Detail closed before reopening the field from the graph");
      await pointerGapClick(browser, await visibleCenter(browser, await comment().getElement()));
      await waitFor(browser, async () => (await field(browser))?.of === "core", "core's field open once more");
      // Emptied, the comment is removed.
      await fill(browser, 'textarea[aria-label="Comment on core"]', "");
      await browser.keys("Enter");
      await waitFor(browser, () => !/annotations/.test(file(fixture, "core") ?? ""), "the empty comment removed from core's file");
      await waitFor(browser, async () => !(await comments(browser))["a:core"], "nothing drawn under core");
      await waitFor(browser, () => browser.execute(() => (document.activeElement?.closest(".svelte-flow__node") as HTMLElement | null)?.dataset.id === "a:core"), "focus back on core's node");

      // Dragged onto docs, which has none, kit's comment moves there.
      const kitComment = await visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:bundle/a:kit")} .comment`).getElement());
      const docs = await visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:docs")}`).getElement());
      const liftedComment = (point: { x: number; y: number }) => browser.execute((to, text) => {
        const lifted = document.querySelector<HTMLElement>(".canvas .lifted");
        if (!lifted || lifted.innerText !== text || !lifted.checkVisibility({ opacityProperty: true, visibilityProperty: true })) return false;
        const at = lifted.getBoundingClientRect();
        return at.width > 0 && at.height > 0 && Math.abs(at.left - to.x) <= 3 && Math.abs(at.top - to.y) <= 3;
      }, point, KIT_NOTE);
      await drag(browser, kitComment, { x: docs.x, y: docs.y }, {
        held: async () => {
          await waitFor(browser, () => targeted(browser, "a:docs"), "docs highlighted as the drop target");
          assert.equal((await comments(browser))["a:bundle/a:kit"], undefined, "the comment's place left empty while it is lifted");
          await waitFor(browser, () => liftedComment(docs), "the lifted text is painted at the held pointer");
          const next = { x: Math.round(docs.x + 12), y: Math.round(docs.y + 4) };
          await dispatchInput(browser, { kind: "pointer", phase: "move", ...next });
          await waitFor(browser, () => liftedComment(next), "the lifted text follows the held pointer to a second position");
        },
      });
      await waitFor(browser, () => commentIn(fixture, "docs") === KIT_NOTE && !/annotations/.test(file(fixture, "kit") ?? ""), "the comment moved from kit's file to docs's");
      // Onto app, which has one, it is no drop target and returns; Esc puts one back too. The
      // graph is first drawn again for the moved comment.
      const docsComment = async () => visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:docs")} .comment`).getElement());
      await waitFor(browser, async () => (await comments(browser))["a:docs"] === KIT_NOTE && !("a:bundle/a:kit" in (await comments(browser))), "the moved comment drawn under docs");
      const appBox = await settled(browser, "a:app");
      // Onto app's title row: app itself, not a part in its box.
      const beforeOccupiedDrop = [file(fixture, "docs"), file(fixture, "app")];
      await drag(browser, await docsComment(), { x: appBox.left + 60, y: appBox.top + 14 }, { held: async () => assert.equal(await targeted(browser, "a:app"), false, "app has a comment: no target") });
      await afterRender(browser);
      await waitFor(browser, async () => (await comments(browser))["a:docs"] === KIT_NOTE && (await comments(browser))["a:app"] === APP_NOTE &&
        !(await browser.$(".lifted").isDisplayed()), "the comment is drawn back under docs and app keeps its comment");
      assert.deepEqual([file(fixture, "docs"), file(fixture, "app")], beforeOccupiedDrop, "releasing over an occupied target changes neither file");
      assert.equal(commentIn(fixture, "docs"), KIT_NOTE, `the comment stays with docs: ${["app", "kit", "core", "bundle"].map((name) => `${name}=${JSON.stringify(commentIn(fixture, name))}`).join(" ")}`);
      // The other no-target releases also restore the same drawing, without a disk edit.
      const allDefinitions = () => ["app", "bundle", "core", "docs", "kit"].map((name) => file(fixture, name));
      const noDropFiles = allDefinitions();
      const own = await settled(browser, "a:docs");
      for (const [where, point] of [["its own node", own], ["the background", await below(browser, own.x)]] as const) {
        await drag(browser, await docsComment(), point, {
          held: async () => {
            await waitFor(browser, () => liftedComment(point), `the comment is held over ${where}`);
            assert.equal(await browser.$(`${CURRENT} .artifact.target, ${CURRENT} .whole.target`).isExisting(), false, `${where} is not a drop target`);
          },
        });
        await waitFor(browser, async () => (await comments(browser))["a:docs"] === KIT_NOTE && !(await browser.$(".lifted").isDisplayed()),
          `the comment is visible back under docs after release over ${where}`);
        assert.deepEqual(allDefinitions(), noDropFiles, `release over ${where} writes nothing`);
      }
      // Held over core, which would take it, Esc puts it back before the button is let go; the
      // release then writes nothing.
      const coreNow = (await box(browser, "a:core"))!;
      const unmoved = ["docs", "core"].map((name) => file(fixture, name));
      await drag(browser, await docsComment(), { x: coreNow.x, y: coreNow.y }, {
        held: async () => waitFor(browser, () => targeted(browser, "a:core"), "core highlighted as the drop target"),
        escape: true,
        escaped: async () => {
          await waitFor(browser, async () => (await comments(browser))["a:docs"] === KIT_NOTE && !(await browser.$(".lifted").isExisting()) && !(await targeted(browser, "a:core")),
            "Esc put the comment back under docs, nothing lifted or highlighted, the button still down");
        },
      });
      await afterRender(browser);
      assert.deepEqual(["docs", "core"].map((name) => file(fixture, name)), unmoved, "the release after Esc wrote nothing");

      // Repeat over an expanded whole and a minimized whole using bundle, which has no comment.
      const bundleTo = () => titleOf(browser, "a:bundle");
      const bundleComment = () => browser.$(`${CURRENT} ${nodeAt("a:bundle")} .comment`);
      const moveComment = async (from: "docs" | "bundle", to: "docs" | "bundle", start: { x: number; y: number }, end: { x: number; y: number }) => {
        await drag(browser, start, end, { held: () => waitFor(browser, () => targeted(browser, `a:${to}`), `${to} highlighted`) });
        await waitFor(browser, async () => commentIn(fixture, from) === null && commentIn(fixture, to) === KIT_NOTE &&
          (await comments(browser))[`a:${to}`] === KIT_NOTE && !(await comments(browser))[`a:${from}`], `the comment moves from ${from} to ${to} on disk and on screen`);
      };
      await moveComment("docs", "bundle", await docsComment(), await bundleTo());
      await moveComment("bundle", "docs", await visibleCenter(browser, await bundleComment().getElement()), await settled(browser, "a:docs"));
      const bundleButton = (action: string) => browser.$(`${CURRENT} ${nodeAt("a:bundle")} button[aria-label^="${action} bundle"]`);
      await bundleButton("Minimize").click();
      await bundleButton("Expand").waitForDisplayed({ timeout: 15000 });
      await moveComment("docs", "bundle", await docsComment(), await settled(browser, "a:bundle"));
      await moveComment("bundle", "docs", await visibleCenter(browser, await bundleComment().getElement()), await settled(browser, "a:docs"));
      await bundleButton("Expand").click();
      await graphShown(browser, "a:bundle/a:kit");

      // On the branch's Run the comments are drawn alike, and are no handle: a drag pans the
      // camera, a click opens the Artifact's Detail.
      const before = ["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name));
      await showRun(browser, "repo  master", run);
      await graphShown(browser, "a:docs");
      await waitFor(browser, async () => JSON.stringify(await comments(browser)) === JSON.stringify({ "a:app": APP_NOTE, "a:docs": KIT_NOTE }), "the Run drawn with the definitions' comments");
      const onRun = await places(browser);
      const runCamera = await camera(browser);
      const runComment = await visibleCenter(browser, await browser.$(`.run ${nodeAt("a:docs")} .comment`).getElement());
      await drag(browser, runComment, { x: runComment.x + 120, y: runComment.y + 60 }, {
        held: async () => assert.equal(await browser.execute(() => document.querySelector(".artifact.target, .whole.target") !== null), false, "no drop target on a Run"),
      });
      await waitFor(browser, async () => (await camera(browser)) !== runCamera, "the drag on a comment panned the camera");
      assert.deepEqual(await places(browser), onRun, "moving nothing against another");
      assert.deepEqual(["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name)), before, "nothing written");
      await clickAt(browser, await visibleCenter(browser, await browser.$(`.run ${nodeAt("a:docs")} .comment`).getElement()));
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact docs", "a click on a Run's comment opens its Artifact's Detail");
      assert.equal(await field(browser), null, "and no comment field");
      await browser.$(DETAIL).$("button=Close").click();

      // A file changed on disk refuses a comment, saying which; the field stays with its text.
      await chooseChip(browser, "repo  master", "current");
      await currentShown(browser, "a:docs");
      await pointerGapClick(browser, await docsComment());
      await waitFor(browser, async () => (await field(browser))?.of === "docs", "docs's field open");
      writeText(nativePath(fixture.repo, ".artifactize", "docs.artf"), `${file(fixture, "docs")}\n# changed`);
      await browser.keys(" again");
      await browser.keys("Enter");
      await waitFor(browser, async () => (await notice(browser)) === ".artifactize/docs.artf changed on disk.", "the changed file refused");
      assert.ok(file(fixture, "docs")?.endsWith("# changed"), "the other writer's text stays");
      assert.equal((await field(browser))?.text, `${KIT_NOTE} again`, "the field stays with its text");
      await press(browser, ["Escape"], async () => (await field(browser)) === null, "the field closed");
      const coreDrop = await settled(browser, "a:core");
      const externallyAdded = "Core now has an externally added comment.";
      const untouchedSource = file(fixture, "docs");
      const editedTarget = `${file(fixture, "core")}\n[annotations.comment]\ntext = ${JSON.stringify(externallyAdded)}\n`;
      await drag(browser, await docsComment(), coreDrop, {
        held: async () => {
          await waitFor(browser, () => targeted(browser, "a:core"), "core is highlighted before the external edit");
          writeText(pathOf(fixture, "core"), editedTarget);
        },
      });
      await waitFor(browser, async () => (await notice(browser)) === ".artifactize/core.artf changed on disk.", "the refused drop names the externally edited target");
      await waitFor(browser, async () => (await comments(browser))["a:docs"] === KIT_NOTE && (await comments(browser))["a:core"] === externallyAdded,
        "the refreshed graph shows the source comment back and the external edit on the target");
      assert.deepEqual([file(fixture, "docs"), file(fixture, "core")], [untouchedSource, editedTarget], "the conflicting drop overwrote neither definition");
    },
    async (browser) => {
      // After a restart the comments are drawn as the definitions hold them.
      await currentShown(browser, "a:docs");
      await waitFor(browser, async () => (await comments(browser))["a:app"] === APP_NOTE && (await comments(browser))["a:docs"] === "Check the kit\nthen the bundle", "the comments drawn after a restart");
      assert.equal((await commentOf(browser, "a:app"))?.quiet, true, "in the quieter ink");
    },
  ),
];
