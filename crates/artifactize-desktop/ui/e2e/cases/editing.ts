// Editing a branch's Current: an Artifact's Definition and Source in its Detail, New Artifact,
// Rename and Delete, places by hand and parts by dragging on the graph, and comments. Each case
// ends in what the window shows and what the definition files on disk hold.
import assert from "node:assert/strict";
import { git } from "../environment.ts";
import { prepareFixture } from "../build-fixtures.ts";
import { verifyAll, type Fixture } from "../fixture.ts";
import { runtimeProfile } from "../native-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { exists, makeDirectory, nativePath, primaryModifier, readText, writeText } from "../platform.ts";
import { detail, graphShown, nodeAt, openArtifact } from "../graph-view.ts";
import { MAIN, mainTitle, scanned, showRun } from "../pane.ts";
import { afterRender, fill, press } from "../keys.ts";
import { clickAt, visibleCenter } from "../pointer.ts";
import { dispatchInput } from "../input.ts";

const CURRENT = 'section[aria-label="Current"]';
const DETAIL = 'aside[aria-label="Detail"]';
const TOOLS = '[aria-label="Graph tools"]';

/** The definitions of the editing fixture: app and bundle each hold kit, docs reads core.
 * `comments` adds a comment to app and one to core. */
function definitions(comments: boolean): Record<string, string> {
  const comment = (text: string) => (comments ? `\n[annotations.comment]\ntext = ${JSON.stringify(text)}\n` : "");
  return {
    app: `path = "app/"\nparts = ["kit"]\nbasis = true\n${comment("The app is split next.")}`,
    bundle: `path = "bundle/"\nparts = ["kit"]\nbasis = true\n`,
    kit: `path = "kit/"\nbasis = true\n`,
    core: `path = "core/"\nbasis = true\n${comment("Core needs a review.")}`,
    docs: `path = "docs/"\n\n[evals.check]\ntitle = "The docs read the core"\nprofile = ${runtimeProfile()}\npayload.instruction = "Check {docs} against {core}."\n`,
  };
}

/** A branch whose definitions are in `.artifactize/`, one file each, without a Run. */
function createEditing(comments = false): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    const folder = makeDirectory(nativePath(repo, ".artifactize"));
    for (const [name, text] of Object.entries(definitions(comments))) {
      writeText(nativePath(makeDirectory(nativePath(repo, name)), "readme.md"), `${name}\n`);
      writeText(nativePath(folder, `${name}.artf`), text);
    }
    git(repo, "init", "-q", "--initial-branch=master", repo);
    return { root, repo, state: nativePath(root, "state") };
  });
}

/** The definition file of `name` as it is on disk now, or `null` once it is gone. */
const file = (fixture: Fixture, name: string) => {
  const path = nativePath(fixture.repo, ".artifactize", `${name}.artf`);
  return exists(path) ? readText(path) : null;
};

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

/** Every drawn copy's place on the shown graph, from docs's, in the graph's own units (a node
 * is 168 wide), whatever the camera's zoom. */
const places = (browser: WebdriverIO.Browser) => browser.execute(() => {
  const nodes = [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].filter((node) => !node.closest("aside"));
  const origin = nodes.find((node) => node.dataset.id === "a:docs")?.getBoundingClientRect();
  const unit = (nodes.find((node) => node.querySelector(".artifact"))?.getBoundingClientRect().width ?? 168) / 168;
  return Object.fromEntries(nodes.map((node) => {
    const at = node.getBoundingClientRect();
    return [node.dataset.id ?? "", [Math.round((at.left - (origin?.left ?? 0)) / unit), Math.round((at.top - (origin?.top ?? 0)) / unit)]];
  }));
});

/** A point of the Current's canvas below every drawn Artifact, `x` across: free space. */
const below = (browser: WebdriverIO.Browser, x: number) => browser.execute((root, x) => {
  const canvas = document.querySelector(`${root} [data-graph-area]`)!.getBoundingClientRect();
  const bottom = Math.max(...[...document.querySelectorAll(`${root} .svelte-flow__node`)].map((node) => node.getBoundingClientRect().bottom));
  return { x, y: Math.round((bottom + canvas.bottom) / 2) };
}, CURRENT, x);

/** Whether the drawn copy `id` is outlined as the drop target. */
const targeted = (browser: WebdriverIO.Browser, id: string) => browser.execute((selector) => !!document.querySelector(`${selector} .artifact.target, ${selector} .whole.target`), `${CURRENT} ${nodeAt(id)}`);

/** Press at `from`, move to `to` in steps and release there, through the suite's one input
 * helper: the embedded provider has no pointerdown or pointerup of its own. `held` runs while
 * the button is still down; `escape` presses Esc then. */
async function drag(
  browser: WebdriverIO.Browser,
  from: { x: number; y: number },
  to: { x: number; y: number },
  { held, escape = false }: { held?: () => Promise<void>; escape?: boolean } = {},
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
    marked: [...section.querySelectorAll(".margin li.marked")].map((line) => line.textContent?.trim() ?? ""),
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
  return open ? { of: open.getAttribute("aria-label")?.replace("Comment on ", "") ?? "", text: open.value, focused: document.activeElement === open } : null;
});

export const scenarios: Scenario[] = [
  // UX journeys: J-DEFINITION J-DEFINITION-EDIT J-PARTS-EDIT J-DEFINITION-CONFLICT
  // UX edges: E-CURRENT-EVALS E-DETAIL-SECTION E-DEFINITION-PATH E-DEFINITION-ESC E-SOURCE-TYPE E-SOURCE-SAVE
  // E-DEFINITION-INVALID E-SOURCE-REVERT E-PARTS-PICK E-PARTS-ADD E-PARTS-REMOVE E-PARTS-REFUSED E-EDIT-CONFLICT E-EDIT-UNSAVED
  scenario<Record<string, never>>(
    "editing-definition",
    "shows and edits an Artifact's Definition and Source on the Current, refusing what the project would not accept, a changed file and a drag over typed text",
    async () => {
      const fixture = createEditing();
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

      // Typing in the Source offers Save and Revert; Ctrl+S writes it.
      const text = `${DETAIL} textarea[aria-label="Source of core"]`;
      await typeIn(browser, text, "\n# reviewed");
      await waitFor(browser, async () => (await source(browser))?.save === true, "Save offered for typed text");
      assert.equal(file(fixture, "core")?.includes("# reviewed"), false, "nothing written before Save");
      await press(browser, [primaryModifier, "s"], () => file(fixture, "core")?.endsWith("# reviewed") ?? false, "Ctrl+S wrote the Source");
      await waitFor(browser, async () => (await source(browser))?.save === false, "Save gone once written");
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
      await waitFor(browser, async () => (await browser.$(`${CURRENT} ${nodeAt("a:app/a:core")}`).isExisting()), "the graph draws core in app's box");
      await browser.$(`${DETAIL} button[aria-label="Remove core from the parts of app"]`).click();
      await waitFor(browser, () => !/"core"/.test(file(fixture, "app") ?? ""), "core gone from app's parts");
      await waitFor(browser, async () => (await browser.$(`${CURRENT} ${nodeAt("a:core")}`).isExisting()), "the graph draws core outside app");
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
      // A drag over typed text is refused before anything is written; the typed text stays
      // with core while its Detail is closed.
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed");
      const core = (await box(browser, "a:core"))!;
      await drag(browser, { x: core.x, y: core.y }, { x: core.x + 60, y: core.y + 40 });
      await waitFor(browser, async () => (await notice(browser)).startsWith(".artifactize/core.artf has typed text"), "a move over typed text refused");
      assert.equal(file(fixture, "core"), theirs, "nothing written");
      assert.deepEqual(await box(browser, "a:core"), core, "core back where it was");
      await openArtifact(browser, "a:core", "Artifact core");
      await waitFor(browser, async () => (await source(browser))?.text.endsWith("# mine") ?? false, "the typed text still in core's Source");
      await browser.$(DETAIL).$("button=Revert").click();
      await waitFor(browser, async () => (await source(browser))?.text === theirs, "Revert shows what is on disk");
      // Saved, the next move is written.
      await typeIn(browser, text, "\n# ours");
      await browser.$(DETAIL).$("button=Save").click();
      await waitFor(browser, () => file(fixture, "core")?.endsWith("# ours") ?? false, "the Source saved on what is on disk");
    },
  ),

  // UX journeys: J-NODE-NEW J-NODE-RENAME J-NODE-DELETE
  // UX edges: E-NODE-NEW-OPEN E-EDIT-DIALOG-INVALID E-EDIT-DIALOG-CANCEL E-NODE-NEW E-NODE-RENAME-OPEN E-NODE-RENAME
  // E-NODE-DELETE-OPEN E-NODE-DELETE-REFUSED E-NODE-DELETE
  scenario<Record<string, never>>(
    "editing-nodes",
    "creates an Artifact from the tool strip, renames one with its references, and deletes one only while nothing names it",
    async () => {
      const fixture = createEditing();
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await currentShown(browser, "a:core");
      const dialog = () => browser.$('[role="dialog"]');
      const refusal = () => dialog().$('[role="alert"]');
      // New Artifact: a name already used, ignoring case, is refused under its field.
      await browser.$(TOOLS).$("button=New Artifact").click();
      await dialog().waitForExist({ timeout: 15000, timeoutMsg: "the New Artifact dialog" });
      assert.equal(await dialog().$(".dialog-title").getText(), "New Artifact");
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "Core");
      await fill(browser, '[role="dialog"] label:nth-of-type(2) input', "core/");
      await dialog().$("button=Create").click();
      await refusal().waitForExist({ timeout: 15000, timeoutMsg: "a used name refused" });
      assert.equal(file(fixture, "Core"), null, "nothing written");
      await browser.keys("Escape");
      await dialog().waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "Esc closes the dialog" });
      // Created: the file is written, the node drawn and its Detail opened.
      await browser.$(TOOLS).$("button=New Artifact").click();
      await dialog().waitForExist({ timeout: 15000 });
      await fill(browser, '[role="dialog"] label:nth-of-type(1) input', "extra");
      await fill(browser, '[role="dialog"] label:nth-of-type(2) input', "extra/");
      await browser.keys("Enter");
      await waitFor(browser, () => file(fixture, "extra") !== null, "extra's file written");
      assert.ok(file(fixture, "extra")?.includes('path = "extra/"'), "with its Path");
      await graphShown(browser, "a:extra");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact extra", "the new Artifact's Detail open");

      // Rename: the file and every reference follow in one edit.
      await openArtifact(browser, "a:core", "Artifact core");
      await browser.$(DETAIL).$("button=Rename").click();
      await dialog().waitForExist({ timeout: 15000 });
      assert.equal(await dialog().$(".dialog-title").getText(), "Rename core");
      assert.equal(await dialog().$("input").getValue(), "core", "the field holds the name");
      await fill(browser, '[role="dialog"] input', "base");
      await dialog().$("button=Rename").click();
      await waitFor(browser, () => file(fixture, "base") !== null && file(fixture, "core") === null, "core.artf renamed base.artf");
      assert.ok(file(fixture, "docs")?.includes("{base}"), "docs's reference follows");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact base", "the Detail names base");
      await graphShown(browser, "a:base");

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
      await openArtifact(browser, "a:extra", "Artifact extra");
      await browser.$(DETAIL).$("button=Delete").click();
      await dialog().waitForExist({ timeout: 15000 });
      await dialog().$("button=Delete").click();
      await waitFor(browser, () => file(fixture, "extra") === null, "extra's file removed");
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "its Detail closed");
      await waitFor(browser, async () => !(await browser.$(`${CURRENT} ${nodeAt("a:extra")}`).isExisting()), "its node gone");
    },
  ),

  // UX journeys: J-NODE-MOVE J-LAYOUT-RESET J-PARTS-DRAG J-DEFINITION
  // UX edges: E-NODE-DRAG E-NODE-PLACE E-NODE-DRAG-CANCEL E-PARTS-DROP E-PARTS-DROP-OUT E-PARTS-DROP-REFUSED E-LAYOUT-RESET
  // E-LAYOUT-SAVED E-RUN-NO-EDIT E-GRAPH-CAMERA E-PANE-CHIP E-MON-DETAIL-ARTIFACT
  scenario<{ run: string }>(
    "editing-layout",
    "places Artifacts by hand and makes parts by dropping them, saving places in the definitions, which draw the branch's Run too and hold after a restart",
    async () => {
      const fixture = createEditing();
      const verified = await verifyAll(fixture);
      if (verified.code !== 0) throw new Error(`the editing fixture did not verify: ${verified.stdout}`);
      const { id: run } = JSON.parse(verified.stdout) as { id: string };
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { run } };
    },
    async (browser, { fixture, data: { run } }) => {
      await currentShown(browser, "a:core");
      assert.equal(await browser.$(TOOLS).$("button=Reset layout").isExisting(), false, "the top level is laid out automatically");
      // A press and release without moving opens the Detail.
      const core = (await box(browser, "a:core"))!;
      await clickAt(browser, { x: Math.round(core.x), y: Math.round(core.y) });
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact core", "a click without movement opens the Detail");
      await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed, the whole graph in reach");
      // Esc during a drag puts the node back, writing nothing.
      const files = () => ["core", "docs", "app", "bundle", "kit"].map((name) => file(fixture, name));
      const untouched = files();
      await drag(browser, { x: core.x, y: core.y }, { x: core.x + 80, y: core.y + 80 }, { escape: true });
      await afterRender(browser);
      assert.deepEqual(await box(browser, "a:core"), core, "core back where it was");
      assert.deepEqual(files(), untouched, "nothing written");
      // A drag places it: the first move writes every top-level node's place, in its own file.
      const moved = await below(browser, core.x + 40);
      await drag(browser, { x: core.x, y: core.y }, moved);
      await waitFor(browser, () => ["core", "docs", "app", "bundle"].every((name) => /\bat = \[/.test(file(fixture, name) ?? "")), "every top-level node's place written");
      assert.equal(/annotations/.test(file(fixture, "kit") ?? ""), false, "a part has no top-level place");
      await waitFor(browser, async () => (await browser.$(TOOLS).$("button=Reset layout").isExisting()), "Reset layout offered for the hand-placed top level");
      const placed = await box(browser, "a:core");
      // The node follows the pointer from where the drag began, a few pixels past the press.
      assert.ok(placed && Math.abs(placed.x - moved.x) < 12 && Math.abs(placed.y - moved.y) < 12, `core stays where it was dropped: ${JSON.stringify(placed)} for ${JSON.stringify(moved)}`);
      // A later move writes the moved node's file only.
      const docsBefore = file(fixture, "docs");
      await drag(browser, placed!, { x: placed!.x + 40, y: placed!.y });
      await waitFor(browser, async () => Math.abs(((await box(browser, "a:core"))?.x ?? 0) - (placed!.x + 40)) < 12, "core moved again");
      assert.equal(file(fixture, "docs"), docsBefore, "docs's file untouched");

      // Dropped into app's box, core becomes app's part; app's places are written as shown. The
      // box is highlighted while the drop would be accepted.
      const appTitle = await titleOf(browser, "a:app");
      const into = (await box(browser, "a:app"))!;
      const at = (await box(browser, "a:core"))!;
      // Onto app's title row: inside its box, over none of its parts.
      await drag(browser, at, { x: into.left + 60, y: into.top + 14 }, {
        held: async () => waitFor(browser, () => targeted(browser, "a:app"), "app's box highlighted as the drop target"),
      });
      await waitFor(browser, () => /parts = \[[^\]]*"core"/.test(file(fixture, "app") ?? ""), "app's file names core as a part");
      await waitFor(browser, () => /\bcore = \[/.test(file(fixture, "app") ?? "") && /\bkit = \[/.test(file(fixture, "app") ?? ""), "core's place in app's box written, and kit's as shown");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app/a:core")}`).isExisting(), "core drawn in app's box");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label="Reset layout of app"]`).isExisting(), "the hand-placed box offers Reset layout");
      // Dropped out of every box, it leaves app; its own place is written.
      const part = (await box(browser, "a:app/a:core"))!;
      const app = (await box(browser, "a:app"))!;
      await drag(browser, part, await below(browser, app.left + app.width + 60));
      await waitFor(browser, () => !/"core"/.test(file(fixture, "app") ?? ""), "core gone from app's parts");
      await waitFor(browser, async () => await browser.$(`${CURRENT} ${nodeAt("a:core")}`).isExisting(), "core drawn at the top level");
      // A drop that would make a cycle: app onto the copy of its own part kit in bundle.
      const kitInBundle = (await box(browser, "a:bundle/a:kit"))!;
      const appBefore = file(fixture, "app");
      const kitBefore = file(fixture, "kit");
      const where = await titleOf(browser, "a:app");
      await drag(browser, where, { x: kitInBundle.x, y: kitInBundle.y }, {
        held: async () => assert.equal(await targeted(browser, "a:bundle/a:kit"), false, "a cycle is no drop target"),
      });
      await waitFor(browser, async () => /part/.test(await notice(browser)), "the cycle refused with its reason");
      assert.equal(file(fixture, "app"), appBefore, "app's file untouched");
      assert.equal(file(fixture, "kit"), kitBefore, "kit's file untouched");
      assert.ok(appTitle, "app's title row is where a whole is dragged from");

      // Reset layout: app's box first, then the top level.
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label="Reset layout of app"]`).click();
      await waitFor(browser, () => !/annotations\.style\.parts/.test(file(fixture, "app") ?? ""), "app's places gone");
      await browser.$(TOOLS).$("button=Reset layout").click();
      await waitFor(browser, () => ["core", "docs", "app", "bundle"].every((name) => !/\bat = \[/.test(file(fixture, name) ?? "")), "every top-level place gone");
      await waitFor(browser, async () => !(await browser.$(TOOLS).$("button=Reset layout").isExisting()), "Reset layout gone with the places");

      // Placed again, the places draw the branch's Run as they draw its Current.
      const again = (await box(browser, "a:docs"))!;
      await drag(browser, again, await below(browser, again.x + 50));
      await waitFor(browser, () => /\bat = \[/.test(file(fixture, "docs") ?? ""), "docs placed");
      await afterRender(browser);
      const onCurrent = await places(browser);
      await showRun(browser, "repo  master", run);
      await graphShown(browser, "a:docs");
      let onRunPlaces = {};
      await waitFor(browser, async () => JSON.stringify(onRunPlaces = await places(browser)) === JSON.stringify(onCurrent), "the Run's graph drawn with the same places")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(onRunPlaces)} on the Run, ${JSON.stringify(onCurrent)} on the Current`); });
      // A Run's graph is a record: nothing moves, nothing is offered to edit.
      const onRun = (await box(browser, "a:docs"))!;
      await drag(browser, onRun, { x: onRun.x + 80, y: onRun.y + 80 });
      await afterRender(browser);
      assert.equal(await browser.$(TOOLS).$("button=New Artifact").isExisting(), false, "no New Artifact on a Run");
      assert.equal(await browser.$(TOOLS).$("button=Reset layout").isExisting(), false, "no Reset layout on a Run");
      const after = await places(browser);
      assert.deepEqual(after, onCurrent, "the drag on a Run panned the camera, moving no node against another");
      await browser.$(nodeAt("a:docs")).click();
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact docs", "a Run's Artifact opens its Detail");
      await waitFor(browser, async () => await browser.$(`${DETAIL} section[aria-label="Path"]`).isExisting(), "a Run's Detail shows its Path");
      assert.equal(await browser.$(`${DETAIL} section[aria-label="Definition"]`).isExisting(), false, "and no Definition");
    },
    async (browser) => {
      // After a restart the places still hold.
      await currentShown(browser, "a:docs");
      await waitFor(browser, async () => await browser.$(TOOLS).$("button=Reset layout").isExisting(), "the top level still placed by hand");
    },
  ),

  // UX journeys: J-COMMENT J-COMMENT-EDIT J-COMMENT-MOVE
  // UX edges: E-COMMENT-DRAWN E-COMMENT-OPEN E-COMMENT-WRITE E-COMMENT-CLICK E-COMMENT-ESC E-COMMENT-REMOVE E-COMMENT-DRAG
  // E-COMMENT-DROP E-COMMENT-DROP-NONE E-COMMENT-DRAG-CANCEL E-GRAPH-MINIMIZE E-GRAPH-EXPAND E-EDIT-CONFLICT
  scenario<Record<string, never>>(
    "editing-comments",
    "draws each comment under its node, writes one from the Definition's Comment or a click on it, and moves one by dragging",
    async () => {
      const fixture = createEditing(true);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await currentShown(browser, "a:core");
      await waitFor(browser, async () => JSON.stringify(await comments(browser)) === JSON.stringify({ "a:app": "The app is split next.", "a:core": "Core needs a review." }),
        `each comment under its node or its whole's title row: ${JSON.stringify(await comments(browser))}`);
      // Minimized, a whole keeps its comment under it.
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Minimize app"]`).click();
      await waitFor(browser, async () => (await comments(browser))["a:app"] === "The app is split next.", "the minimized whole's comment under it");
      await browser.$(`${CURRENT} ${nodeAt("a:app")} button[aria-label^="Expand app"]`).click();
      await graphShown(browser, "a:app/a:kit");

      // The Definition's Comment opens the field under kit; Enter writes it, focus goes back.
      await openArtifact(browser, "a:bundle/a:kit", "Artifact kit");
      await browser.$(DETAIL).$("button=Comment").click();
      await waitFor(browser, async () => (await field(browser))?.of === "kit" && (await field(browser))?.focused === true, "kit's field open and focused");
      await browser.keys("Check the kit");
      await browser.keys("Enter");
      await waitFor(browser, () => /\[annotations\.comment\]\s*text = "Check the kit"/.test(file(fixture, "kit") ?? ""), "kit's comment written in its file");
      assert.equal(file(fixture, "kit")?.startsWith('path = "kit/"\nbasis = true\n'), true, "nothing else in kit's file changed");
      await waitFor(browser, async () => (await comments(browser))["a:app/a:kit"] === "Check the kit" && (await comments(browser))["a:bundle/a:kit"] === "Check the kit", "drawn under every copy of kit");
      await waitFor(browser, async () => await browser.$(`${DETAIL} button[data-comment-button]`).isFocused(), "focus back on Comment");

      // A click on core's comment opens it; Esc puts it back; an empty field removes it.
      const comment = () => browser.$(`${CURRENT} ${nodeAt("a:core")} .comment`);
      await clickAt(browser, await visibleCenter(browser, await comment().getElement()));
      await waitFor(browser, async () => (await field(browser))?.text === "Core needs a review.", "core's field holds its comment");
      await browser.keys(" now");
      await browser.keys("Escape");
      await waitFor(browser, async () => (await field(browser)) === null, "Esc closes the field");
      assert.ok(file(fixture, "core")?.includes('text = "Core needs a review."'), "nothing written");
      await clickAt(browser, await visibleCenter(browser, await comment().getElement()));
      await waitFor(browser, async () => (await field(browser))?.of === "core", "core's field open again");
      // Only a space left: an empty comment.
      await fill(browser, 'textarea[aria-label="Comment on core"]', " ");
      await browser.keys("Enter");
      await waitFor(browser, () => !/annotations/.test(file(fixture, "core") ?? ""), "the empty comment removed from core's file");
      await waitFor(browser, async () => !(await comments(browser))["a:core"], "nothing drawn under core");

      // Dragged onto docs, which has none, kit's comment moves there.
      if (await browser.$(DETAIL).isExisting()) await browser.$(DETAIL).$("button=Close").click();
      await waitFor(browser, async () => !(await browser.$(DETAIL).isExisting()), "the Detail closed, the whole graph in reach");
      const kitComment = await visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:bundle/a:kit")} .comment`).getElement());
      const docs = (await box(browser, "a:docs"))!;
      await drag(browser, kitComment, { x: docs.x, y: docs.y }, { held: async () => waitFor(browser, () => targeted(browser, "a:docs"), "docs highlighted as the drop target") });
      await waitFor(browser, () => /text = "Check the kit"/.test(file(fixture, "docs") ?? "") && !/annotations/.test(file(fixture, "kit") ?? ""), "the comment moved from kit's file to docs's");
      // Onto app, which has one, it is no drop target and returns; Esc puts one back too.
      const docsComment = async () => visibleCenter(browser, await browser.$(`${CURRENT} ${nodeAt("a:docs")} .comment`).getElement());
      const appBox = (await box(browser, "a:app"))!;
      await drag(browser, await docsComment(), { x: appBox.x, y: appBox.top + 10 }, { held: async () => assert.equal(await targeted(browser, "a:app"), false, "app has a comment: no target") });
      await afterRender(browser);
      assert.ok(/text = "Check the kit"/.test(file(fixture, "docs") ?? ""), "the comment stays with docs");
      await drag(browser, await docsComment(), { x: docs.x + 200, y: docs.y }, { escape: true });
      await afterRender(browser);
      assert.equal((await comments(browser))["a:docs"], "Check the kit", "Esc put the comment back");

      // A file changed on disk refuses a comment, saying which.
      await clickAt(browser, await docsComment());
      await waitFor(browser, async () => (await field(browser))?.of === "docs", "docs's field open");
      writeText(nativePath(fixture.repo, ".artifactize", "docs.artf"), `${file(fixture, "docs")}\n# changed`);
      await browser.keys(" again");
      await browser.keys("Enter");
      await waitFor(browser, async () => (await notice(browser)) === ".artifactize/docs.artf changed on disk.", "the changed file refused");
      assert.ok(file(fixture, "docs")?.endsWith("# changed"), "the other writer's text stays");
    },
  ),
];
