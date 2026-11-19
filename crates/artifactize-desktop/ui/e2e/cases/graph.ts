// A Run's graph in the Main area, in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them and follows a submission made by
// another process live without moving any node while release's Detail stays open; a click on an
// Artifact opens its Detail with a tab per eval, docked at the graph's right edge, whose camera ignores it,
// and the camera brings a large Run's Artifacts into view.
import assert from "node:assert/strict";
import { addWorktrees, artifactize, create, createChain, createOutside, createEvals, createJoined, createParts, requests, savedRuns, signOff, signoff } from "../fixture.ts";
import { afterRender, fill, press, typeText } from "../keys.ts";
import { clickAt, visibleCenter } from "../pointer.ts";
import { buttonKey, compositionKey, dispatchInput, tab } from "../input.ts";
import {
  aside, camera, detail, dockedDetail, fact, FIND, findField, fitButton, focusedElement, focusedNode, graphShown, NO_FAVORITES, nodeAt, offCentre, openArtifact, openArtifactTab, openTab, summary, tabAt,
  unfold, ZOOM_IN, ZOOM_OUT,
} from "../graph-view.ts";
import { git } from "../environment.ts";
import { runtimeProfile } from "../native-fixtures.ts";
import { nativeInput, primaryModifier } from "../platform.ts";
import { makeDirectory, nativePath, writeText } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { canvasSnapshot } from "../rendering.ts";
import { graphAppearance, graphHasColours, visibleColour, type GraphAppearance } from "./graph-appearance.ts";
import { branch, chooseChip, ENTRIES, entriesFocused, listed, MAIN, mainFocused, mainTitle, scanned, searchFocused, showRun } from "../pane.ts";

/** A fixture whose one Run ended: the branch's newest entry is its Current, so its Run is chosen. */
function ended(fixture: ReturnType<typeof createChain>) {
  const [run] = savedRuns(fixture);
  return { fixture, args: ["--repo", fixture.repo], data: { run: run.id } };
}

async function shownRun(browser: WebdriverIO.Browser, run: string) {
  await listed(browser, "repo  master");
  await showRun(browser, "repo  master", run);
}

/** What the Main area's graph says at its bottom, if anything. */
function hint(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => document.querySelector(".graph > .canvas > .hint")?.textContent?.trim() ?? "");
}

/** The star beside the open Detail's title of the Artifact `name`: as drawn (`★` filled, `☆`
 * outlined), and whether it is on. */
async function star(browser: WebdriverIO.Browser, name: string): Promise<{ glyph: string; on: boolean }> {
  const button = aside(browser).$(`button[aria-label=${JSON.stringify(`Star ${name}`)}]`);
  await button.waitForExist({ timeout: 15000, timeoutMsg: `${name}'s star beside the Detail's title` });
  const icon = await button.$("svg").getAttribute("data-icon");
  return { glyph: icon === "star-on" ? "★" : icon === "star" ? "☆" : String(icon), on: (await button.getAttribute("aria-pressed")) === "true" };
}

/** Click the star of the open Detail of `name` and wait until it is `on`, or off. */
async function toggleStar(browser: WebdriverIO.Browser, name: string, on: boolean): Promise<void> {
  await star(browser, name);
  await aside(browser).$(`button[aria-label=${JSON.stringify(`Star ${name}`)}]`).click();
  await browser.waitUntil(async () => JSON.stringify(await star(browser, name)) === JSON.stringify({ glyph: on ? "★" : "☆", on }),
    { timeout: 15000, timeoutMsg: `${name} ${on ? "starred" : "no longer starred"}` });
}

/** Arrow keys on the focused Artifact `id` are no keys of the graph's: its camera stays, the
 * Artifact stays where it was drawn, and the keyboard stays on it. */
async function arrowsMoveNothing(browser: WebdriverIO.Browser, id: string): Promise<void> {
  const at = () => browser.execute((want) => [...document.querySelectorAll(`.svelte-flow__node[data-id="${want}"]`)]
    .find((node) => !node.closest('aside[aria-label="Detail"]'))?.getBoundingClientRect().toJSON(), id);
  const [before, where] = [await camera(browser), await at()];
  // Each key on its own, once the page has rendered it: a move one key undoes cannot hide.
  for (const key of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]) {
    await press(browser, [key], () => true, `${key} reached the page`);
    await afterRender(browser);
    assert.equal(await camera(browser), before, `${key} moved no camera`);
    assert.deepEqual(await at(), where, `${key} moved no Artifact`);
    assert.equal(await focusedNode(browser), id, `the keyboard stays on the Artifact after ${key}`);
  }
}

/** The copies of {@link createParts}'s graph: the wholes suite and docs, app inside suite, kit
 * inside app, site inside both, and lib, which docs reads, and guide, which reads suite, outside
 * every whole. */
const PARTS = ["a:suite", "a:suite/a:app", "a:suite/a:app/a:kit", "a:suite/a:site", "a:docs", "a:docs/a:site", "a:lib", "a:guide"];

/** {@link PARTS} with suite minimized: suite a node, its parts hidden, site still inside docs. */
const SUITE_MINIMIZED = ["a:suite", "a:docs", "a:docs/a:site", "a:lib", "a:guide"];

/** Whether exactly `keys` are the copies drawn under `root` now, outside the Detail. */
async function drawnNow(browser: WebdriverIO.Browser, root: string, keys: string[]): Promise<boolean> {
  const drawn = await browser.execute((under) => [...document.querySelectorAll<HTMLElement>(`${under} .svelte-flow__node`)]
    .filter((node) => !node.closest('aside[aria-label="Detail"]'))
    .map((node) => node.dataset.id ?? ""), root);
  return JSON.stringify(drawn.sort()) === JSON.stringify([...keys].sort());
}

/** The copies of the graph under `root`, outside the Detail, once exactly `keys` are drawn:
 * each one's box, whether it is drawn as a box or minimized, its title text, its Artifact, its
 * spoken name, whether it shows as opened, and its light. */
async function partsDrawn(browser: WebdriverIO.Browser, root: string, keys: string[] = PARTS) {
  const read = () => browser.execute((under) => Object.fromEntries([...document.querySelectorAll<HTMLElement>(`${under} .svelte-flow__node`)]
    .filter((node) => !node.closest('aside[aria-label="Detail"]'))
    .map((node) => [node.dataset.id ?? "", {
      rect: node.getBoundingClientRect().toJSON() as DOMRect,
      whole: node.classList.contains("svelte-flow__node-whole"),
      minimized: node.classList.contains("svelte-flow__node-minimized"),
      text: node.querySelector(".text, .name")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
      tally: node.querySelector(".tally")?.textContent?.trim() ?? "",
      artifact: node.dataset.artifact ?? "",
      label: node.getAttribute("aria-label") ?? "",
      open: !!node.querySelector(".open"),
      light: node.dataset.light ?? "",
    }])), root);
  let shown: Awaited<ReturnType<typeof read>> = {};
  await browser.waitUntil(async () => {
    shown = await read();
    return JSON.stringify(Object.keys(shown).sort()) === JSON.stringify([...keys].sort());
  }, { timeout: 15000, timeoutMsg: `exactly ${keys.join(", ")} drawn` }).catch((error: unknown) => {
    throw new Error(`${String(error)}; drawn: ${Object.keys(shown).join(", ")}`);
  });
  return shown;
}

/** Where the drawn edge `id` under `root`, outside the Detail, starts and ends on the screen:
 * its path's points under the camera's transform, from the canvas's corner; null while it is
 * not drawn. */
function edgeEnds(browser: WebdriverIO.Browser, root: string, id: string) {
  return browser.execute((under, want) => {
    const path = [...document.querySelectorAll<SVGPathElement>(`${under} .svelte-flow__edge[data-id="${want}"] .svelte-flow__edge-path`)]
      .find((each) => !each.closest('aside[aria-label="Detail"]'));
    const [flow, viewport] = [path?.closest(".svelte-flow"), path?.closest<HTMLElement>(".svelte-flow__viewport")];
    if (!path || !flow || !viewport) return null;
    const corner = flow.getBoundingClientRect();
    const camera = new DOMMatrix(getComputedStyle(viewport).transform);
    const at = (length: number) => {
      const point = camera.transformPoint(new DOMPoint(path.getPointAtLength(length).x, path.getPointAtLength(length).y));
      return { x: corner.left + point.x, y: corner.top + point.y };
    };
    return { start: at(0), end: at(path.getTotalLength()) };
  }, root, id);
}

/** A point on the `side` edge of a box, within a few pixels. */
const meets = (point: { x: number; y: number }, box: DOMRect, side: "left" | "right") =>
  Math.abs(point.x - box[side]) <= 4 && point.y >= box.top && point.y <= box.bottom;

/** Drag the background of the graph under `root`, left of the Detail, until the button named
 * `name` lies clear of the Detail: as far as the window allows each time, a few times at most. */
async function panClear(browser: WebdriverIO.Browser, root: string, name: string) {
  const steps: unknown[] = [];
  // A bound on the drags a narrow window needs, not a claimed number.
  for (let drag = 0; drag < 4; drag += 1) {
    const step = await browser.execute((under, want) => {
      const canvas = document.querySelector(`${under} > .canvas`)?.getBoundingClientRect();
      const cover = document.querySelector('aside[aria-label="Detail"]')?.getBoundingClientRect();
      const button = document.querySelector(`${under} .svelte-flow__node button[aria-label="${want}"]`)?.getBoundingClientRect();
      if (!canvas || !cover || !button) return { failure: "the canvas, the Detail and the button drawn" };
      const shift = button.right - (cover.left - 24);
      if (shift <= 1) return null;
      for (let x = Math.min(cover.left, canvas.right) - 24; x > canvas.left + 24; x -= 16) {
        for (let y = canvas.bottom - 24; y > canvas.top + 48; y -= 16) {
          if (document.elementFromPoint(x, y)?.classList.contains("svelte-flow__pane")) {
            return { x: Math.round(x), y: Math.round(y), by: Math.ceil(Math.min(shift, x - canvas.left - 12)) };
          }
        }
      }
      return { failure: "background left of the Detail to drag" };
    }, root, name);
    steps.push(step);
    if (step === null) return;
    if ("failure" in step) assert.fail(`no pan clears ${name}: ${step.failure}`);
    await dispatchInput(browser, { kind: "pointer", phase: "down", x: step.x, y: step.y });
    await afterRender(browser);
    await dispatchInput(browser, { kind: "pointer", phase: "move", x: step.x - step.by, y: step.y });
    await dispatchInput(browser, { kind: "pointer", phase: "up", x: step.x - step.by, y: step.y });
    await afterRender(browser);
  }
  assert.fail(`${name} still under the Detail after the drags: ${JSON.stringify(steps)}`);
}

/** Two screen positions at most a pixel apart. */
const near = (one: DOMRect, other: DOMRect) => Math.abs(one.left - other.left) <= 1 && Math.abs(one.top - other.top) <= 1;

/** Click the button named `name` in the graph under `root` at its centre. */
async function clickButton(browser: WebdriverIO.Browser, root: string, name: string) {
  await clickAt(browser, await visibleCenter(browser, await browser.$(`${root} .svelte-flow__node button[aria-label="${name}"]`).getElement()));
}

/** A rectangle wholly inside another. */
const within = (inner: DOMRect, outer: DOMRect) =>
  inner.left >= outer.left && inner.top >= outer.top && inner.right <= outer.right && inner.bottom <= outer.bottom;

/** The whole `key` under `root` shows `name` and its mark in a title row at the top of its box,
 * above every one of its `parts`, the strip closing that row in its mark's colour while its
 * outline stays neutral. */
async function titledAtTop(browser: WebdriverIO.Browser, root: string, key: string, name: string, parts: string[]) {
  const drawn = await partsDrawn(browser, root);
  const title = await browser.execute((under, want) => {
    const node = [...document.querySelectorAll(`${under} .svelte-flow__node[data-id="${want}"]`)].find((found) => !found.closest('aside[aria-label="Detail"]'));
    const whole = node?.querySelector<HTMLElement>(".whole");
    const [text, dot, row] = [whole?.querySelector<HTMLElement>(".name"), whole?.querySelector<HTMLElement>(".light"), whole?.querySelector<HTMLElement>(".title")];
    if (!whole || !text || !dot || !row) return null;
    // The colour the title row's strip is painted in, however it is drawn: an inset shadow, a
    // bottom border unlike its other sides, or a bottom pseudo-element's background.
    const style = getComputedStyle(row);
    const inset = style.boxShadow.split(/,(?![^(]*\))/).find((part) => part.includes("inset"))?.match(/rgba?\([^)]*\)|color\([^)]*\)/)?.[0];
    const after = getComputedStyle(row, "::after");
    const strip = inset
      ?? (Number.parseFloat(style.borderBottomWidth) > 0 && style.borderBottomColor !== style.borderTopColor ? style.borderBottomColor : undefined)
      ?? (after.content !== "none" ? after.backgroundColor : "");
    return {
      strip,
      box: whole.getBoundingClientRect().toJSON() as DOMRect,
      name: text.getBoundingClientRect().toJSON() as DOMRect,
      dot: dot.getBoundingClientRect().toJSON() as DOMRect,
      text: text.textContent?.trim() ?? "",
      colour: getComputedStyle(dot).color,
      border: getComputedStyle(whole).borderTopColor,
    };
  }, root, key);
  assert.ok(title, `${key} is drawn with a title`);
  assert.equal(title.text, name, `${key} shows its name`);
  for (const [what, rect] of [["name", title.name], ["light", title.dot]] as const) {
    assert.ok(within(rect, title.box) && parts.every((part) => rect.bottom <= drawn[part].rect.top),
      `${key}'s ${what} sits at the top of its box, above its parts: ${JSON.stringify({ rect, box: title.box })}`);
  }
  assert.ok(visibleColour(title.colour) && title.colour === title.strip && title.colour !== title.border,
    `${key}'s mark and strip in one colour, its outline neutral: ${title.colour} / ${title.strip} / ${title.border}`);
}

/** Two rectangles that do not meet. */
const apart = (one: DOMRect, other: DOMRect) =>
  one.left >= other.right || one.right <= other.left || one.top >= other.bottom || one.bottom <= other.top;

/** A click at the top left of the whole `key`'s box, on its name, outside its parts. */
async function clickWhole(browser: WebdriverIO.Browser, root: string, key: string) {
  const at = await browser.execute((under, want) => {
    const box = [...document.querySelectorAll(`${under} .svelte-flow__node[data-id="${want}"]`)]
      .find((node) => !node.closest('aside[aria-label="Detail"]'))!.getBoundingClientRect();
    return { x: Math.round(box.left + 10), y: Math.round(box.top + 8) };
  }, root, key);
  await browser.action("pointer").move({ x: at.x, y: at.y }).down().up().perform();
}

/** Visible bodies, keyed by visible section headers in the open Detail. */
function detailSections(browser: WebdriverIO.Browser) {
  return browser.execute(() => Object.fromEntries([...document.querySelectorAll('aside[aria-label="Detail"] .section')]
    .flatMap((section) => {
      const header = section.querySelector(".head");
      if (!header?.checkVisibility({ opacityProperty: true, visibilityProperty: true })) return [];
      const body = section.querySelector(":scope > .body");
      const box = body?.getBoundingClientRect();
      return [[header.textContent?.trim() ?? "", !!box && box.width > 0 && box.height > 0
        && body!.checkVisibility({ opacityProperty: true, visibilityProperty: true })]];
    })));
}

export const scenarios: Scenario[] = [
  scenario<Record<string, never>>(
    "graph-live-verdicts",
    "lights Artifacts and edges, recolours them in place with an Artifact's Detail open and shows the submitted eval's tab",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const graph = () => graphAppearance(browser);
      await graphShown(browser, "a:release");
      // Each comparison uses the very drawing that met its wait: the graph may redraw again
      // before a second reading.
      let drawn: GraphAppearance | undefined;
      await browser.waitUntil(async () => {
        const shown = await graph();
        if (!graphHasColours(shown, { "a:release": "active", "a:ship": "waiting", "a:docs": "green" }, { "a:release>a:ship": "active" })) return false;
        drawn = shown;
        return true;
      }, { timeout: 15000, timeoutMsg: "release, ship, docs and the relation are painted in their verdict's own colours" });
      assert.ok(drawn);
      const before = drawn;
      /** release's node as drawn: its mark's icon and its tally. */
      const releaseNode = () => browser.execute(() => {
        const node = [...document.querySelectorAll('.svelte-flow__node[data-id="a:release"]')].find((element) => !element.closest('aside[aria-label="Detail"]'));
        return [node?.querySelector(".light svg")?.getAttribute("data-icon") ?? "", node?.querySelector(".tally")?.textContent?.trim() ?? ""];
      });
      assert.deepEqual(await releaseNode(), ["running", "0/1"], "release in progress, none of its one eval GREEN");

      // A click opens release's Detail on its Artifact tab: its one eval, waiting for a Human.
      const opened = await openArtifact(browser, "a:release", "Artifact release");
      assert.deepEqual(opened.tabs, [], "the Artifact's own tab has no chips");
      assert.deepEqual(opened.evals.map((row) => row.name), ["signoff"], "the Artifact tab lists release's evals");
      await dockedDetail(browser, opened.title);
      // On the eval's tab the chips read as a path: release's own chip, then its one eval's.
      await openTab(browser, "e:release/signoff");
      const onTab = (await detail(browser))!;
      assert.deepEqual(onTab.tabs.map((tab) => tab.name), ["release", "signoff"], "release's own chip, then its one eval's");
      assert.equal(onTab.path, "release / signoff", "the chips read as a path");
      assert.equal(onTab.tabs[0].label, "Artifact release", "release's chip is read as the Artifact release");
      await browser.waitUntil(async () => (await detail(browser))?.tabs[0]?.icon === true,
        { timeout: 15000, timeoutMsg: "release's chip draws the Artifact icon before its name" });
      assert.equal(onTab.tabs[1].glyph, "human", "the sign-off's chip shows it waiting for a Human");
      await openArtifactTab(browser, "release");

      // Another process signs the release off; the change hint recolours the graph and the tab.
      const id = signoff(fixture).id;
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "alice"), 0);
      assert.equal(
        artifactize(fixture, "request", "submit", id, "--reviewer", "alice", "--verdict", "GREEN", "--fields", '{"approved":true,"comment":"ok"}'),
        0,
      );
      let recoloured: GraphAppearance | undefined;
      await browser.waitUntil(async () => {
        const shown = await graph();
        if (!graphHasColours(shown, { "a:release": "green", "a:docs": "green" }, { "a:release>a:ship": "green" })) return false;
        recoloured = shown;
        return true;
      }, { timeout: 15000, timeoutMsg: "the submitted release's node mark and strip are painted GREEN, its relation neutral" });
      assert.ok(recoloured);
      const after = recoloured;
      assert.notEqual(after.nodes["a:release"].mark, before.nodes["a:release"].mark, "the result changed the actual node mark colour");
      assert.notEqual(after.nodes["a:release"].strip, before.nodes["a:release"].strip, "the result changed the actual node strip colour");
      assert.equal(after.edges["a:release>a:ship"].stroke, before.edges["a:release>a:ship"].stroke, "the relation stays neutral");
      for (const node of ["a:release", "a:ship", "a:docs"]) {
        assert.equal(after.nodes[node].at, before.nodes[node].at, `${node} kept its place`);
      }
      await browser.waitUntil(async () => JSON.stringify(await releaseNode()) === JSON.stringify(["green", "1/1"]),
        { timeout: 15000, timeoutMsg: `release's mark and tally updated in place: ${JSON.stringify(await releaseNode())}` });
      const row = () => browser.execute(() => document.querySelector('aside[aria-label="Detail"] button[data-eval="e:release/signoff"] .glyph svg')?.getAttribute("data-icon") ?? "");
      await browser.waitUntil(async () => (await row()) === "green", { timeout: 15000, timeoutMsg: "the open Detail's sign-off row turned GREEN" });
      assert.equal((await detail(browser))?.title, "Artifact release", "a live update keeps the open Detail on release");

      // The sign-off's tab holds its completed Human review; Esc closes the Detail.
      await openTab(browser, "e:release/signoff");
      assert.equal((await detail(browser))?.tabs[1]?.glyph, "green", "its chip GREEN");
      const review = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"][tabindex="0"]');
      await browser.waitUntil(async () => (await review().isExisting()) && /completed result/i.test(await aside(browser).getText()),
        { timeout: 15000, timeoutMsg: "the completed review on the sign-off's tab" });
      await review().click();
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      await graphShown(browser, "a:release");
    },
  ),

  // UX journey: J-GRAPH-COLOURS
  scenario<null>(
    "graph-live-blocking-edge",
    "repaints a previously neutral outgoing edge with its new blocking colour in place",
    async () => { const fixture = await create(); return { fixture, args: ["--repo", fixture.repo], data: null }; },
    async (browser, { fixture }) => {
      const edge = "a:release>a:ship";
      await graphShown(browser, "a:release");
      let neutral: GraphAppearance | undefined;
      await browser.waitUntil(async () => {
        const shown = await graphAppearance(browser);
        if (!graphHasColours(shown, { "a:release": "active" }, { [edge]: "active" })) return false;
        neutral = shown;
        return true;
      }, { timeout: 15000, timeoutMsg: "the outgoing edge is initially painted neutral" });
      assert.ok(neutral);
      const geometry = () => browser.execute((id) => document.querySelector(`.graph .svelte-flow__edge[data-id="${id}"] path`)?.getAttribute("d") ?? "", edge);
      const route = await geometry();
      assert.ok(route);
      const id = signoff(fixture).id;
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "alice"), 0);
      assert.equal(artifactize(fixture, "request", "submit", id, "--reviewer", "alice", "--verdict", "RED", "--fields", '{"reason":"The release is incomplete"}'), 0);
      const before = neutral;

      await browser.waitUntil(async () => {
        const now = await graphAppearance(browser);
        return graphHasColours(now, { "a:release": "red", "a:ship": "blocked" }, { [edge]: "red" }) &&
          now.edges[edge].stroke !== before.edges[edge].stroke && (await geometry()) === route &&
          now.nodes["a:release"].at === before.nodes["a:release"].at && now.nodes["a:ship"].at === before.nodes["a:ship"].at;
      }, { timeout: 15000, timeoutMsg: "the same painted stroke becomes red and its endpoints do not move" });
    },
  ),

  // UX journey: J-DETAIL
  scenario<Record<string, never>>(
    "graph-click-detail",
    "opens an Artifact's Detail on a click or Enter over the graph's right side with the graph visible beside it, switches it on a click on another Artifact, closes it on the background, then steps back to the Pane",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      // A 1440×872 window, which the Detail at its default width leaves room beside.
      await browser.setWindowSize(1440, 872);
      await afterRender(browser);
      await graphShown(browser, "a:release");
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane's entries focused at launch" });
      // Enter steps from the Pane into the Main area.
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      /** Each Artifact's right edge in the window, and the open Detail's left edge (null without one). */
      const placed = () => browser.execute(() => ({
        nodes: Object.fromEntries([...document.querySelectorAll<HTMLElement>(".svelte-flow__node")]
          .map((node) => [node.dataset.id ?? "", node.getBoundingClientRect().right])),
        detail: document.querySelector('aside[aria-label="Detail"]')?.getBoundingClientRect().left ?? null,
      }));
      // Where the Artifacts are drawn once the resized window has settled.
      let whole: Record<string, number> = {};
      await browser.waitUntil(async () => {
        const first = JSON.stringify((await placed()).nodes);
        await afterRender(browser);
        whole = (await placed()).nodes;
        return first === JSON.stringify(whole);
      }, { timeout: 15000, timeoutMsg: "the graph settled in the resized window" });

      // A click opens an Artifact's Detail and leaves the keyboard on that Artifact: Esc closes
      // the Detail, Enter on the focused Artifact opens it again, as the click did.
      await openArtifact(browser, "a:release", "Artifact release");
      await browser.waitUntil(async () => (await focusedNode(browser)) === "a:release", { timeout: 15000, timeoutMsg: "the keyboard on release" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === "Artifact release", "Enter on release opened its Detail again");

      // The Detail is a panel docked at the graph's right edge without a gap, where another
      // Artifact was drawn without it, and the graph stays visible on its left.
      await dockedDetail(browser, "Artifact release");
      const edge = (await placed()).detail!;
      const under = Object.keys(whole).filter((id) => id !== "a:release" && whole[id] > edge);
      assert.ok(under.length > 0, `the Detail docks where an Artifact was drawn: ${JSON.stringify({ whole, edge })}`);
      const beside = await placed();
      const exposed = Object.keys(beside.nodes).filter((id) => id !== "a:release" && beside.nodes[id] < beside.detail!);
      assert.ok(exposed.length > 0, `another Artifact stays visible beside the Detail: ${JSON.stringify(beside)}`);
      // A click on an Artifact shown beside it switches the Detail to that Artifact, and a click
      // on the graph's background closes it. Esc then steps back to the Pane.
      await openArtifact(browser, exposed[0], `Artifact ${exposed[0].slice(2)}`);
      const canvas = await browser.execute(() => {
        const box = document.querySelector(".graph .canvas")!.getBoundingClientRect();
        const nodes = [...document.querySelectorAll(".svelte-flow__node")].map((node) => node.getBoundingClientRect());
        // A point of the background below every node, left of the Detail and right of the
        // camera controls.
        const below = Math.max(...nodes.map((node) => node.bottom));
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        return { x: Math.round((box.left + dock.left) / 2), y: Math.round(Math.min(box.bottom - 20, below + (box.bottom - below) / 2)) };
      });
      await browser.action("pointer").move({ x: canvas.x, y: canvas.y }).down().up().perform();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the background click closed the Detail" });
      await press(browser, ["Escape"], () => entriesFocused(browser), "back to the Pane");
      assert.equal((await branch(browser, "repo  master"))?.selected, true, "the same branch stays selected");
    },
  ),

  // UX journey: J-GRAPH-PARTS
  scenario<{ run: string }>(
    "graph-parts",
    "draws each whole of a Run as a box around its parts, nested as deep as they go, a shared part in each of its wholes, edges into a box like any other, and opens the whole's Detail from its box and a part's from the part",
    async () => ended(createParts()),
    async (browser, { data }) => {
      await shownRun(browser, data.run);
      await graphShown(browser, "a:suite/a:app/a:kit");
      const drawn = await partsDrawn(browser, ".graph");
      assert.deepEqual(Object.keys(drawn).sort(), [...PARTS].sort(), "each Artifact once, site once in each of its wholes");
      for (const [key, whole] of [["a:suite", true], ["a:suite/a:app", true], ["a:docs", true], ["a:suite/a:site", false], ["a:docs/a:site", false], ["a:suite/a:app/a:kit", false], ["a:lib", false]] as const) {
        assert.equal(drawn[key].whole, whole, `${key} is ${whole ? "" : "not "}drawn as a box`);
      }
      for (const [part, whole] of [["a:suite/a:app", "a:suite"], ["a:suite/a:app/a:kit", "a:suite/a:app"], ["a:suite/a:site", "a:suite"], ["a:docs/a:site", "a:docs"]]) {
        assert.ok(within(drawn[part].rect, drawn[whole].rect), `${part} sits in ${whole}'s box`);
      }
      assert.ok(apart(drawn["a:suite"].rect, drawn["a:docs"].rect), "the wholes' boxes do not meet");
      for (const whole of ["a:suite", "a:docs"]) assert.ok(apart(drawn["a:lib"].rect, drawn[whole].rect), `lib stands outside ${whole}`);
      assert.ok(apart(drawn["a:suite/a:app"].rect, drawn["a:suite/a:site"].rect), "suite's parts do not meet");
      // A whole shows its name at its top, lit as its parts are, and is read aloud as a node is.
      assert.equal(drawn["a:suite"].text, "suite");
      assert.equal(drawn["a:suite"].light, "green");
      assert.match(drawn["a:suite"].label, /^suite: GREEN(, \d+ of \d+ evals GREEN)?$/);
      await titledAtTop(browser, ".graph", "a:suite", "suite", ["a:suite/a:app", "a:suite/a:site"]);
      await titledAtTop(browser, ".graph", "a:docs", "docs", ["a:docs/a:site"]);
      await browser.waitUntil(async () => graphHasColours(await graphAppearance(browser), { "a:suite": "green", "a:docs": "green" }, {}),
        { timeout: 15000, timeoutMsg: "the wholes lit in their verdict's colour" });
      assert.deepEqual([drawn["a:suite/a:site"].text, drawn["a:docs/a:site"].text], ["site", "site"], "both copies are site");
      // Each copy of site carries site's edges: from lib outside every box into it, and out of
      // it to app, within suite and across from docs into suite.
      await browser.waitUntil(async () => graphHasColours(await graphAppearance(browser), { "a:lib": "green", "a:suite/a:site": "green", "a:docs/a:site": "green" }, {
        "a:lib>a:suite/a:site": "green",
        "a:lib>a:docs/a:site": "green",
        "a:suite/a:site>a:suite/a:app": "green",
        "a:docs/a:site>a:suite/a:app": "green",
        "a:lib>a:docs": "green",
        "a:suite>a:guide": "green",
      }), { timeout: 15000, timeoutMsg: "every copy of site's edges and the wholes' own edges drawn and lit like any other" });
      // Edges to and from a whole meet its box: lib's into docs on its left side, suite's to
      // guide on its right.
      const [into, out] = [await edgeEnds(browser, ".graph", "a:lib>a:docs"), await edgeEnds(browser, ".graph", "a:suite>a:guide")];
      assert.ok(into && meets(into.end, drawn["a:docs"].rect, "left") && meets(into.start, drawn["a:lib"].rect, "right"),
        `lib's edge ends on docs's box: ${JSON.stringify([into, drawn["a:docs"].rect])}`);
      assert.ok(out && meets(out.start, drawn["a:suite"].rect, "right") && meets(out.end, drawn["a:guide"].rect, "left"),
        `suite's edge starts on suite's box: ${JSON.stringify([out, drawn["a:suite"].rect])}`);

      // A click on suite's box outside its parts opens suite; a click on a part opens the part,
      // suite's box staying as it was.
      await clickWhole(browser, ".graph", "a:suite");
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact suite", { timeout: 15000, timeoutMsg: "the click on suite's box opened suite's Detail" });
      const whole = (await detail(browser))!;
      assert.deepEqual(whole.tabs, [], `suite's Detail opens on its Artifact tab: ${JSON.stringify(whole.tabs)}`);
      // Its neighbours are read with the tab: the section shows once they are, as it has some.
      await aside(browser).$('[aria-label="Neighbours"]').waitForExist({ timeout: 15000, timeoutMsg: "suite's Neighbours section" });
      assert.ok(await aside(browser).$('[aria-label="Evals of suite"]').isExisting(),
        "the tab shows the whole's own Artifact content: its neighbours and its evals");
      const suite = (await partsDrawn(browser, ".graph"))["a:suite"].rect;
      await openArtifact(browser, "a:suite/a:app/a:kit", "Artifact kit");
      assert.deepEqual((await partsDrawn(browser, ".graph"))["a:suite"].rect, suite, "suite's box stays");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes kit's Detail");
    },
  ),

  // UX journeys: J-GRAPH-PARTS J-GRAPH-FIND J-DETAIL
  scenario<Record<string, never>>(
    "graph-parts-shared",
    "on a branch's Current, selecting one copy of a part of two wholes highlights both and its Detail names the wholes; the find field lists it once and goes to its first copy",
    async () => {
      // Its Run ended, so the branch's newest entry, shown at launch, is its Current.
      const fixture = createParts();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const current = 'section[aria-label="Current"]';
      await browser.$(`${current} .svelte-flow__node[data-id="a:docs/a:site"]`).waitForExist({ timeout: 15000, timeoutMsg: "the Current's graph drawn with its wholes" });
      const drawn = await partsDrawn(browser, current);
      assert.ok(within(drawn["a:docs/a:site"].rect, drawn["a:docs"].rect) && within(drawn["a:suite/a:site"].rect, drawn["a:suite"].rect), "the Current draws site in both wholes");
      // Each whole shows its name and light at the top of its box, as on a Run graph.
      await titledAtTop(browser, current, "a:suite", "suite", ["a:suite/a:app", "a:suite/a:site"]);
      await titledAtTop(browser, current, "a:docs", "docs", ["a:docs/a:site"]);

      // From the Pane, Enter gives the graph the keyboard; Tab reaches each whole before its
      // parts, opening nothing, and Enter opens the focused whole's or part's Detail.
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      const order: string[] = [];
      // A bound on traversing this small fixture's controls, not a claimed tab order.
      for (let attempt = 0; attempt < 60 && order.length < PARTS.length; attempt += 1) {
        await tab(browser);
        const focused = await focusedNode(browser);
        if (focused === null || order.includes(focused)) continue;
        assert.ok(PARTS.includes(focused), `Tab reached a copy of the graph: ${focused}`);
        assert.equal(await detail(browser), null, `focusing ${focused} opens nothing`);
        order.push(focused);
        const opens: Record<string, string | undefined> = { "a:suite": "suite", "a:suite/a:app": "app", "a:suite/a:app/a:kit": "kit" };
        const name = opens[focused];
        if (name === undefined) continue;
        // Enter on a whole or on a part opens that Artifact's own Detail, on its Artifact tab.
        await press(browser, ["Enter"], async () => {
          const shown = await detail(browser);
          return shown?.title === `Artifact ${name}` && shown.tabs.length === 0 && shown.evals.length > 0;
        }, `Enter on ${focused} opens ${name}'s Detail on its Artifact tab`);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, `${name}'s Detail closes`);
      }
      assert.deepEqual([...order].sort(), [...PARTS].sort(), "Tab reaches every whole and every part");
      for (const [whole, part] of [["a:suite", "a:suite/a:app"], ["a:suite/a:app", "a:suite/a:app/a:kit"], ["a:suite", "a:suite/a:site"], ["a:docs", "a:docs/a:site"]]) {
        assert.ok(order.indexOf(whole) < order.indexOf(part), `Tab reaches ${whole} before its part ${part}: ${order.join(", ")}`);
      }

      // The find field lists site once; Enter brings its first copy to the middle of the graph.
      await press(browser, [primaryModifier, "f"], async () => (await findField(browser))?.focused === true, "the find field open over the Current");
      await fill(browser, FIND, "site");
      await browser.waitUntil(async () => JSON.stringify((await findField(browser))?.entries.map((entry) => entry.name)) === JSON.stringify(["site"]), { timeout: 15000, timeoutMsg: "site listed once" });
      const first = await browser.execute((under) => [...document.querySelectorAll<HTMLElement>(`${under} .svelte-flow__node[data-artifact="a:site"]`)][0]?.dataset.id ?? "", current);
      await press(browser, ["Enter"], async () => (await findField(browser)) === null && (await detail(browser))?.title === "Artifact site", "Enter opens site's Detail");
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, first);
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4;
      }, { timeout: 15000, timeoutMsg: `site's first copy ${first} in the middle of the graph left of the Detail` });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes site's Detail");

      // A click on docs's copy of site opens site, both copies highlighted.
      await openArtifact(browser, "a:docs/a:site", "Artifact site");
      await unfold(browser, "Debug");
      await browser.waitUntil(async () => {
        const shown = await partsDrawn(browser, current);
        return shown["a:docs/a:site"].open && shown["a:suite/a:site"].open && !shown["a:docs"].open && !shown["a:suite"].open;
      }, { timeout: 15000, timeoutMsg: "both copies of site, and no whole, highlighted" });
      const drawnIn = await browser.execute(() => [...document.querySelectorAll('aside[aria-label="Detail"] dt')]
        .find((key) => key.textContent?.trim() === "Drawn in")?.nextElementSibling?.textContent?.trim() ?? "");
      assert.ok(/\bsuite\b/.test(drawnIn) && /\bdocs\b/.test(drawnIn), `site's Debug says where it is drawn, naming both wholes: ${JSON.stringify(drawnIn)}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes the Current's Detail");
      assert.ok(!(await partsDrawn(browser, current))["a:suite/a:site"].open, "no copy stays highlighted");
    },
  ),

  // UX journeys: J-GRAPH-MINIMIZE J-GRAPH-CAMERA
  scenario<{ run: string }>(
    "graph-minimize",
    "minimizes a whole of a Run to a node that says how many parts it has, the edges of what it hides meeting it, its corner kept on the screen and an open Detail kept; opens it as a node, expands and minimizes it with m while its Detail is open, leaves it as it is when m is typed in a field, during a composition or with focus in the Pane, finds what it hides, keeps it minimized on the branch's Current, expands it there, minimizes and expands it there with m while it is focused, expands it on the Run, and draws every whole as a box again after the app quits",
    async () => ended(createParts()),
    async (browser, { data }) => {
      await shownRun(browser, data.run);
      await graphShown(browser, "a:suite/a:app/a:kit");
      await openArtifact(browser, "a:suite/a:app/a:kit", "Artifact kit");
      await openTab(browser, "e:kit/check");
      const kitDetail = await detail(browser);
      // The Detail is docked over the graph's right side, where suite's Minimize button is: drags on
      // the background, left of the Detail, pan suite out from under it.
      await panClear(browser, ".graph", "Minimize suite (m)");
      const boxed = await partsDrawn(browser, ".graph");

      // With kit's Detail open on its eval's tab, Minimize turns suite into a node of a node's
      // size, its corner where the box's was; site stays drawn inside docs, and the Detail stays
      // as it was, suite highlighted while it hides kit.
      await clickButton(browser, ".graph", "Minimize suite (m)");
      const small = await partsDrawn(browser, ".graph", SUITE_MINIMIZED);
      const suite = small["a:suite"];
      assert.deepEqual([suite.minimized, suite.whole], [true, false], "suite is drawn minimized, not as a box");
      assert.ok(suite.text.includes("suite") && suite.text.includes("2 parts"), `suite shows its name and its two parts: ${suite.text}`);
      assert.match(suite.label, /^suite: GREEN(, \d+ of \d+ evals GREEN)?, minimized, 2 parts$/);
      assert.equal(suite.light, "green");
      assert.ok(Math.abs(suite.rect.width - small["a:lib"].rect.width) <= 1 && Math.abs(suite.rect.height - small["a:lib"].rect.height) <= 1,
        `suite is a node's size: ${JSON.stringify([suite.rect, small["a:lib"].rect])}`);
      await browser.waitUntil(async () => near((await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"].rect, boxed["a:suite"].rect),
        { timeout: 15000, timeoutMsg: "suite's top left corner stays where its box's was on the screen" });
      assert.ok(within(small["a:docs/a:site"].rect, small["a:docs"].rect), "site stays inside docs");
      const kept = await detail(browser);
      assert.deepEqual([kept?.title, kept?.tabs.find((each) => each.selected)?.name], [kitDetail?.title, "check"], "kit's Detail stays on its eval's tab");
      assert.ok(suite.open, "suite shows kit, which it hides, opened");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes kit's Detail");
      // The edges of what suite hides meet it: lib into its site, docs's site into its app; none
      // is drawn among them.
      let edges: string[] = [];
      await browser.waitUntil(async () => {
        const shown = await graphAppearance(browser);
        edges = Object.keys(shown.edges).sort();
        return graphHasColours(shown, { "a:suite": "green", "a:lib": "green" }, { "a:lib>a:suite": "green", "a:lib>a:docs/a:site": "green", "a:docs/a:site>a:suite": "green" });
      }, { timeout: 15000, timeoutMsg: "the hidden parts' edges meet suite, lit as before" });
      assert.deepEqual(edges, ["a:docs/a:site>a:suite", "a:lib>a:docs", "a:lib>a:docs/a:site", "a:lib>a:suite", "a:suite>a:guide"], "each edge once, none within suite");

      // A click on the minimized suite opens its Detail, as on a node. With it open, m expands
      // suite and m minimizes it again as its buttons do, its corner kept on the screen and its
      // Detail open; Esc closes the Detail.
      await openArtifact(browser, "a:suite", "Artifact suite");
      const folded = (await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"];
      // A press on the Detail's text, as a pointer gives it, puts the keyboard in suite's Detail;
      // m then leaves it on the very element that had it.
      await dispatchInput(browser, { kind: "text-click", ...await visibleCenter(browser, await aside(browser).$("h2").getElement()) });
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]')),
        { timeout: 15000, timeoutMsg: "the press moved the keyboard into suite's Detail" });
      const inDetail = await focusedElement(browser);
      assert.ok(await browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]')), `the keyboard is in suite's Detail: ${inDetail}`);
      // A non-editable Detail cannot host native preedit. The component test covers
      // its m guard; the native shortcut probe below uses the focused Find input.
      await press(browser, ["m"], () => drawnNow(browser, ".graph", PARTS), "m expands suite, whose Detail is open");
      assert.equal(await focusedElement(browser), inDetail, "m with the Detail focused leaves the keyboard where it was");
      const unfolded = await partsDrawn(browser, ".graph");
      assert.ok(unfolded["a:suite"].whole && within(unfolded["a:suite/a:app"].rect, unfolded["a:suite"].rect), "m draws suite as a box around its parts");
      await browser.waitUntil(async () => near((await partsDrawn(browser, ".graph"))["a:suite"].rect, folded.rect),
        { timeout: 15000, timeoutMsg: "m keeps suite's top left corner where it was on the screen" });
      await press(browser, ["m"], () => drawnNow(browser, ".graph", SUITE_MINIMIZED), "m minimizes suite again");
      assert.equal(await focusedElement(browser), inDetail, "m with the Detail focused leaves the keyboard where it was again");
      await browser.waitUntil(async () => near((await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"].rect, folded.rect),
        { timeout: 15000, timeoutMsg: "m keeps suite's top left corner where it was on the screen again" });
      assert.equal((await detail(browser))?.title, "Artifact suite", "suite's Detail stays open");
      // Escape closes Find with this same focus when no native composition is active.
      const openFind = () => press(browser, [primaryModifier, "f"], async () => (await findField(browser))?.focused === true, "the find field open over suite's Detail");
      await openFind();
      await press(browser, ["Escape"], async () => (await findField(browser)) === null, "ordinary Escape closes the focused Find field");
      await openFind();
      // Typed in a field, m is text and does not expand suite.
      await typeText(browser, "m");
      await afterRender(browser);
      assert.ok(await drawnNow(browser, ".graph", SUITE_MINIMIZED), "m typed in the find field leaves suite minimized");
      await nativeInput(browser, { type: "set", text: "ㅡ" });
      await browser.waitUntil(async () => (await findField(browser))?.value === "mㅡ", { timeout: 15000, timeoutMsg: "the engine composes ㅡ in the find field" });
      // Probe the shortcut fields while the relevant target actually has native
      // preedit. Escape's plain-key control above rules out suppressed delivery.
      for (const [composing, keyCode] of [[true, 0], [false, 229]] as const) {
        for (const key of ["m", "Escape"] as const) {
          await compositionKey(browser, key, FIND, composing, keyCode);
          await afterRender(browser);
          assert.deepEqual(await findField(browser).then((field) => [field?.focused, field?.value]), [true, "mㅡ"],
            `${key} during composition (isComposing ${composing}, keyCode ${keyCode}) keeps Find focused and the composition in place`);
          assert.ok(await drawnNow(browser, ".graph", SUITE_MINIMIZED), "suite remains minimized");
        }
      }
      // Rollback proves the text remained native preedit through both probes, rather
      // than merely checking that the same text was visible as ordinary input.
      await nativeInput(browser, { type: "cancel" });
      await browser.waitUntil(async () => (await findField(browser))?.value === "m", { timeout: 15000, timeoutMsg: "cancelling native preedit removed only ㅡ" });
      await nativeInput(browser, { type: "set", text: "ㅡ" });
      await browser.waitUntil(async () => (await findField(browser))?.value === "mㅡ", { timeout: 15000, timeoutMsg: "native preedit restarted in Find" });
      await nativeInput(browser, { type: "commit", text: "ㅡ" });
      await press(browser, ["Escape"], async () => (await findField(browser)) === null, "Esc closes the find field");
      assert.equal((await detail(browser))?.title, "Artifact suite", "suite's Detail stays open after the find field");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes suite's Detail");
      // With focus in the Pane, m minimizes or expands nothing; Enter goes back to the Main area.
      await press(browser, ["Escape"], () => entriesFocused(browser), "Esc steps back to the Pane");
      await press(browser, ["m"], () => true, "m reaches the Pane");
      await afterRender(browser);
      assert.ok(await drawnNow(browser, ".graph", SUITE_MINIMIZED), "m in the Pane leaves suite minimized");
      await press(browser, ["Enter"], () => mainFocused(browser), "Enter goes to the Main area");

      // The find field still lists kit, hidden in suite, and goes to the minimized suite for it.
      await press(browser, [primaryModifier, "f"], async () => (await findField(browser))?.focused === true, "the find field open over the Run");
      await fill(browser, FIND, "kit");
      await browser.waitUntil(async () => JSON.stringify((await findField(browser))?.entries.map((entry) => entry.name)) === JSON.stringify(["kit"]), { timeout: 15000, timeoutMsg: "the hidden Artifact listed" });
      await press(browser, ["Enter"], async () => (await findField(browser)) === null && (await detail(browser))?.title === "Artifact kit", "Enter opens kit's Detail");
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, "a:suite");
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4;
      }, { timeout: 15000, timeoutMsg: "the minimized suite in the middle of the graph left of the Detail" });
      assert.ok((await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"].open, "suite shows kit, which it hides, opened");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes kit's Detail");
      await browser.$(ZOOM_OUT).click();
      await press(browser, [primaryModifier, "f"], async () => (await findField(browser))?.focused === true, "Find reopens over the Run");
      await fill(browser, FIND, "check");
      const hit = browser.$('[aria-label="Found"]').$('li=kit / check');
      await hit.waitForDisplayed({ timeout: 15000, timeoutMsg: "the hidden eval can be chosen" });
      await hit.click();
      await browser.waitUntil(async () => (await findField(browser)) === null && (await detail(browser))?.tabs.some((tab) => tab.name === "check" && tab.selected) === true,
        { timeout: 15000, timeoutMsg: "the hidden eval opens its selected Detail tab" });
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, "a:suite");
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 &&
          await browser.$(`${nodeAt("a:suite")} .title .name`).isDisplayed() &&
          (await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"].open &&
          await browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]'));
      }, { timeout: 15000, timeoutMsg: "the hidden eval focuses its Detail and centers the highlighted whole at readable zoom" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes the eval's Detail");

      // The branch's Current draws suite minimized too, Tab reaching its Expand button after it;
      // a click on Expand draws its box again, its corner kept, opening nothing.
      const current = 'section[aria-label="Current"]';
      await chooseChip(browser, "repo  master", "current");
      const there = (await partsDrawn(browser, current, SUITE_MINIMIZED))["a:suite"];
      assert.ok(there.minimized, "suite is minimized on the branch's Current");
      /** Tab until the graph's suite node itself has the keyboard. */
      const toSuite = async () => {
        // A bound on traversing this small fixture's controls, not a claimed tab order.
        for (let attempt = 0; attempt < 80; attempt += 1) {
          if ((await focusedNode(browser)) === "a:suite" && !(await focusedElement(browser)).startsWith("BUTTON")) return;
          await tab(browser);
        }
        assert.fail("Tab reaches suite");
      };
      await toSuite();
      await tab(browser);
      assert.equal((await focusedElement(browser)).trim(), "BUTTON Expand suite (m)", "Tab reaches suite's Expand button after suite");
      assert.equal(await browser.$(`${current} button[aria-label="Expand suite (m)"]`).getAttribute("title"), "Expand (m)", "its tooltip names m");
      await clickButton(browser, current, "Expand suite (m)");
      const open = await partsDrawn(browser, current);
      assert.ok(open["a:suite"].whole && within(open["a:suite/a:app"].rect, open["a:suite"].rect), "suite is a box around its parts again");
      assert.ok(near(open["a:suite"].rect, there.rect), `suite's top left corner stays where it was: ${JSON.stringify([open["a:suite"].rect, there.rect])}`);
      assert.equal(await detail(browser), null, "expanding opens no Detail");
      // On the box, Tab reaches its Minimize button after suite and before its parts.
      await toSuite();
      await tab(browser);
      assert.equal((await focusedElement(browser)).trim(), "BUTTON Minimize suite (m)", "Tab reaches suite's Minimize button after suite");
      assert.equal(await browser.$(`${current} button[aria-label="Minimize suite (m)"]`).getAttribute("title"), "Minimize (m)", "its tooltip names m");
      await tab(browser);
      assert.ok((await focusedNode(browser))?.startsWith("a:suite/"), `Tab reaches a part of suite next: ${await focusedNode(browser)}`);
      // With suite focused and no Detail open, m minimizes it and m expands it again, the
      // keyboard staying on suite and no Detail opening.
      /** The keyboard is on suite's own node, not on a button or a part inside it. */
      const onSuite = () => browser.execute(() => document.activeElement?.matches('.svelte-flow__node[data-id="a:suite"]') ?? false);
      await toSuite();
      await press(browser, ["m"], () => drawnNow(browser, current, SUITE_MINIMIZED), "m minimizes the focused suite");
      await browser.waitUntil(onSuite, { timeout: 15000, timeoutMsg: "the keyboard stays on the minimized suite itself" });
      await press(browser, ["m"], () => drawnNow(browser, current, PARTS), "m expands the focused suite");
      await browser.waitUntil(onSuite, { timeout: 15000, timeoutMsg: "the keyboard stays on suite's box itself" });
      assert.equal(await detail(browser), null, "m opens no Detail");
      // With suite's button focused, m does what the button does and the keyboard stays on the
      // button, which then reads the other action.
      await tab(browser);
      assert.equal((await focusedElement(browser)).trim(), "BUTTON Minimize suite (m)", "Tab reaches suite's Minimize button");
      await press(browser, ["m"], () => drawnNow(browser, current, SUITE_MINIMIZED), "m minimizes suite from its focused button");
      await browser.waitUntil(async () => (await focusedElement(browser)).trim() === "BUTTON Expand suite (m)", { timeout: 15000, timeoutMsg: "the keyboard stays on suite's button, now Expand" });
      await press(browser, ["m"], () => drawnNow(browser, current, PARTS), "m expands suite from its focused button");
      await browser.waitUntil(async () => (await focusedElement(browser)).trim() === "BUTTON Minimize suite (m)", { timeout: 15000, timeoutMsg: "the keyboard stays on suite's button, Minimize again" });
      assert.equal(await detail(browser), null, "m on the button opens no Detail");

      // Expanded on the Current, suite is a box on the branch's Run too; minimized again there,
      // it stays so until the app quits.
      await showRun(browser, "repo  master", data.run);
      assert.ok((await partsDrawn(browser, ".graph"))["a:suite"].whole, "suite is a box on the Run again");
      await clickButton(browser, ".graph", "Minimize suite (m)");
      assert.ok((await partsDrawn(browser, ".graph", SUITE_MINIMIZED))["a:suite"].minimized, "suite minimized on the Run before the app quits");
    },
    async (browser, { data }) => {
      // A new app draws every whole as a box: minimizing lasted only while the app ran.
      await shownRun(browser, data.run);
      const drawn = await partsDrawn(browser, ".graph");
      assert.ok(drawn["a:suite"].whole && !drawn["a:suite"].minimized, "suite is a box again after the app quit");
    },
  ),

  // UX journey: J-DETAIL
  scenario<Record<string, never>>(
    "graph-keyboard-artifacts",
    "Tabs from the Main area to every Artifact of the Run's graph, opening nothing, and opens the corresponding Detail with Enter, without clicking a node",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await browser.waitUntil(() => browser.$(ENTRIES).isFocused(), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });
      await press(browser, ["Enter"], () => browser.$(MAIN).isFocused(), "the Main area reached by keyboard");
      const expected = new Map([["a:release", "release"], ["a:ship", "ship"], ["a:docs", "docs"]]);
      const visited = new Set<string>();
      // A bound on traversing this small fixture's controls, not a claimed tab order.
      const TAB_ATTEMPTS = 40;
      for (let attempt = 0; attempt < TAB_ATTEMPTS && visited.size < expected.size; attempt += 1) {
        await tab(browser);
        const id = await focusedNode(browser);
        if (!id || visited.has(id)) continue;
        const name = expected.get(id);
        assert.ok(name, `Tab reached the expected Artifact ${id}`);
        assert.equal(await detail(browser), null, "focusing an Artifact opens nothing");
        if (visited.size === 0) await arrowsMoveNothing(browser, id);
        await press(browser, ["Enter"], async () => (await detail(browser))?.title === `Artifact ${name}`, `Enter opens ${name}'s Detail`);
        assert.deepEqual((await detail(browser))?.tabs, [], "on its own tab");
        visited.add(id);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the keyboard-opened Detail closes");
      }
      assert.deepEqual([...visited].sort(), [...expected.keys()].sort(), "every Artifact is reachable with Tab and Enter alone");
    },
  ),

  // UX journey: J-DETAIL
  scenario<Record<string, never>>(
    "graph-keyboard-current",
    "Tabs from the Main area to an Artifact of a branch's Current, opening nothing, and opens its Detail with Enter and closes it with Esc, without clicking a node",
    async () => {
      // Its Runs ended, so the branch's newest entry, shown at launch, is its Current.
      const fixture = createChain(3);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const node = (id: string) => `section[aria-label="Current"] .svelte-flow__node[data-id="${id}"]`;
      await browser.$(node("a:n002")).waitForExist({ timeout: 15000, timeoutMsg: "the Current's graph drawn" });
      await browser.waitUntil(() => browser.$(ENTRIES).isFocused(), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });
      await press(browser, ["Enter"], () => browser.$(MAIN).isFocused(), "the Main area reached by keyboard");
      let reached: string | null = null;
      // A bound on traversing this small fixture's controls, not a claimed tab order.
      for (let attempt = 0; attempt < 40 && reached === null; attempt += 1) {
        await tab(browser);
        reached = await browser.execute(() => document.activeElement?.closest('section[aria-label="Current"] .svelte-flow__node')?.getAttribute("data-id") ?? null);
      }
      assert.ok(reached && /^a:n00[0-2]$/.test(reached), `Tab reached an Artifact of the Current: ${reached}`);
      assert.equal(await detail(browser), null, "focusing an Artifact opens nothing");
      await arrowsMoveNothing(browser, reached!);
      const name = reached!.slice(2);
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === `Artifact ${name}`, `Enter opens ${name}'s Detail over the Current`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Current's Detail closes");
    },
  ),

  scenario<Record<string, never>>(
    "graph-run-detail",
    "opens the Run's Detail from its headline and closes it again",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      const run = (await branch(browser, "repo  master"))?.chips.find((chip) => chip.filled)?.id ?? "";
      assert.match(run, /^run-/);
      await browser.$("button.headline").click();
      const shown = await dockedDetail(browser, `Run ${run}`);
      assert.deepEqual(shown.tabs, [], "the Run's Detail has no Artifact or eval tabs");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      await graphShown(browser, "a:release");
    },
  ),

  // UX journey: J-GRAPH-CAMERA
  scenario<{ request: string }>(
    "graph-camera-stays",
    "shows the whole graph first, then leaves its camera to the person: the Detail and a new result never move it, Zoom out and Fit do",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: { request: signoff(fixture).id } };
    },
    async (browser, { fixture, data: { request } }) => {
      await graphShown(browser, "a:release");
      /** The shown graph's camera, and whether every Artifact is inside its canvas. */
      const view = () => browser.execute(() => {
        const ofGraph = (element: Element) => !element.closest('aside[aria-label="Detail"]');
        const viewport = [...document.querySelectorAll<HTMLElement>(".svelte-flow__viewport")].find(ofGraph)!;
        const canvas = [...document.querySelectorAll(".canvas")].find(ofGraph)!.getBoundingClientRect();
        const nodes = [...document.querySelectorAll(".svelte-flow__node")].filter(ofGraph).map((node) => node.getBoundingClientRect());
        return {
          transform: viewport.style.transform,
          whole: nodes.every((box) => box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom),
        };
      });
      /** The camera once it holds still over a painted frame. */
      const settled = async () => {
        let shown = await view();
        await browser.waitUntil(async () => {
          const before = shown.transform;
          await afterRender(browser);
          shown = await view();
          return before === shown.transform;
        }, { timeout: 15000, timeoutMsg: "the camera held still" });
        return shown;
      };
      const first = await settled();
      assert.ok(first.whole, "the graph shows whole when it first appears");
      /** How wide release is drawn, and how far the graph's middle lies from the canvas's. */
      const placed = () => browser.execute(() => {
        const ofGraph = (element: Element) => !element.closest('aside[aria-label="Detail"]');
        const canvas = [...document.querySelectorAll(".canvas")].find(ofGraph)!.getBoundingClientRect();
        const nodes = [...document.querySelectorAll(".svelte-flow__node")].filter(ofGraph).map((node) => node.getBoundingClientRect());
        const [left, right] = [Math.min(...nodes.map((box) => box.left)), Math.max(...nodes.map((box) => box.right))];
        const [top, bottom] = [Math.min(...nodes.map((box) => box.top)), Math.max(...nodes.map((box) => box.bottom))];
        const release = [...document.querySelectorAll('.svelte-flow__node[data-id="a:release"]')].find(ofGraph)! as HTMLElement;
        return { width: release.getBoundingClientRect().width, unscaled: release.offsetWidth, x: (left + right) / 2 - (canvas.left + canvas.right) / 2, y: (top + bottom) / 2 - (canvas.top + canvas.bottom) / 2 };
      });
      const native = await placed();
      assert.ok(native.unscaled > 0 && Math.abs(native.width - native.unscaled) <= 1 && Math.abs(native.x) <= 2 && Math.abs(native.y) <= 2,
        `the small graph is centered at its nodes' own size on first display: ${JSON.stringify(native)}`);
      const title = await mainTitle(browser);

      // Opening a Detail, switching it to a neighbour and closing it leave the camera.
      await openArtifact(browser, "a:release", "Artifact release");
      assert.equal((await settled()).transform, first.transform, "opening the Detail moved nothing");
      await unfold(browser, "Neighbours");
      await aside(browser).$('.svelte-flow__node[data-id="a:ship"]').click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "the Detail switched to ship" });
      assert.equal((await settled()).transform, first.transform, "switching the Detail moved nothing");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      assert.equal((await settled()).transform, first.transform, "closing the Detail moved nothing");

      // The person zooms out (a small graph first fits at the largest zoom); a new result then
      // relights the graph in place, under their camera, not a fit of its own.
      await browser.$(ZOOM_OUT).click();
      await browser.waitUntil(async () => (await view()).transform !== first.transform, { timeout: 15000, timeoutMsg: "Zoom Out moved the camera" });
      const zoomButton = await settled();
      assert.notEqual(zoomButton.transform, first.transform, "Zoom Out moved the camera");
      const toolbarZoom = async () => Number.parseInt(await browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText(), 10);
      const scale = (transform: string) => Number(/scale\(([^)]+)\)/.exec(transform)?.[1]);
      const smaller = await toolbarZoom();
      const point = await visibleCenter(browser, await browser.$('[data-graph-area]').getElement());
      await browser.action("wheel").scroll({ ...point, deltaX: 0, deltaY: 120 }).perform();
      await browser.waitUntil(async () => {
        const moved = scale((await view()).transform);
        const label = await toolbarZoom();
        return moved < scale(zoomButton.transform) && label < smaller && label === Math.round(moved * 100);
      }, { timeout: 15000, timeoutMsg: "the wheel zooms the main graph and its toolbar displays the same new scale" });
      const zoomed = await settled();
      signOff(fixture, request);
      await browser.waitUntil(() => browser.$('.graph .svelte-flow__node[data-id="a:release"]').getAttribute("data-light").then((light) => light === "green"),
        { timeout: 30000, timeoutMsg: "release relit GREEN" });
      assert.equal(await mainTitle(browser), title, "the same Run's graph stays shown");
      assert.equal((await settled()).transform, zoomed.transform, "a new result kept the person's camera");
      // The Run ends with nothing left needing attention, its headline in the one header row,
      // and the camera still stays.
      await browser.waitUntil(async () => (await mainTitle(browser)) === title && await browser.execute(() => {
        const headline = document.querySelector('section[aria-label="Main area"] button.headline');
        return headline?.querySelector(".status")?.textContent?.trim() === "GREEN" && !headline.querySelector(".need");
      }), { timeout: 30000, timeoutMsg: "the selected Run ended GREEN with nothing needing attention" });
      assert.equal((await settled()).transform, zoomed.transform, "the Run's end kept the person's camera");

      // Opening the Detail and switching it keep their camera.
      await openArtifact(browser, "a:release", "Artifact release");
      assert.equal((await settled()).transform, zoomed.transform, "opening the Detail kept the person's camera");
      await aside(browser).$('.svelte-flow__node[data-id="a:ship"]').click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "the Detail switched to ship" });
      assert.equal((await settled()).transform, zoomed.transform, "the person's camera stays");
      // Widening the Detail by its edge keeps it too.
      const edge = await aside(browser).$('[role="separator"][aria-label="Detail width"]');
      const width = async () => (await aside(browser).getSize()).width;
      const narrow = await width();
      await browser.action("pointer", { id: "edge" }).move({ origin: edge, x: 0, y: 0 }).down().perform(true);
      try {
        const pointer = browser.action("pointer", { id: "edge" });
        for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: -20, y: 0 });
        await pointer.up().perform();
      } finally { await browser.releaseActions(); }
      await browser.waitUntil(async () => (await width()) > narrow + 60, { timeout: 15000, timeoutMsg: "the dragged Detail widened" });
      assert.equal((await settled()).transform, zoomed.transform, "resizing the Detail moved nothing");

      // Fit View, the person's own, shows the whole graph in its whole canvas: the Detail is docked
      // over it and takes no part, so the camera is the graph's first one again.
      const centered = async () => {
        const fit = await placed();
        return fit.unscaled > 0 && Math.abs(fit.width - fit.unscaled) <= 1 && Math.abs(fit.x) <= 2 && Math.abs(fit.y) <= 2;
      };
      await fitButton(browser).click();
      await browser.waitUntil(async () => await centered() && (await view()).whole,
        { timeout: 15000, timeoutMsg: "Fit centers the entire graph without enlarging its nodes past their own size" });
      const fitted = await settled();
      assert.notEqual(fitted.transform, zoomed.transform, "Fit View moved the person's camera");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      assert.equal((await settled()).transform, fitted.transform, "closing the Detail keeps what Fit View showed");
      // The person zooms out again, so the next Fit View has a camera to move back.
      await browser.$(ZOOM_OUT).click();
      assert.notEqual((await settled()).transform, fitted.transform, "Zoom Out moved the fitted camera");
      await fitButton(browser).click();
      await browser.waitUntil(async () => (await settled()).transform === fitted.transform,
        { timeout: 15000, timeoutMsg: "Fit View without the Detail fits as with it open" });

      // A chip whose graph is not shown yet draws it whole: the Current, then the Run again.
      const run = (await branch(browser, "repo  master"))!.chips.find((chip) => chip.id !== "current")!.id;
      await chooseChip(browser, "repo  master", "current");
      await browser.waitUntil(async () => (await mainTitle(browser)).startsWith("Current"), { timeout: 15000, timeoutMsg: "the Current shown" });
      await graphShown(browser, "a:release");
      assert.ok((await settled()).whole, "the Current's graph first shows whole");
      await chooseChip(browser, "repo  master", run);
      await browser.waitUntil(async () => (await mainTitle(browser)) === title, { timeout: 15000, timeoutMsg: "the Run shown again" });
      await graphShown(browser, "a:release");
      assert.ok((await settled()).whole, "the Run's graph first shows whole again");
    },
  ),

  scenario<{ run: string }>(
    "graph-chain-culling",
    "draws the Artifacts the camera brings into view when zooming over a chain too large to draw at once",
    async () => {
      // Even a wide desktop needs a scale below 5% to fit this chain in the split canvas.
      return ended(createChain(125));
    },
    async (browser, { data: { run } }) => {
      await shownRun(browser, run);
      const name = (index: number) => `a:n${String(index).padStart(3, "0")}`;
      /** The chain's Artifacts drawn now and wholly inside the canvas. */
      const visible = () => browser.execute(() => {
        const canvas = document.querySelector(".graph .canvas")?.getBoundingClientRect();
        if (!canvas) return [];
        return [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].filter((node) => {
          const box = node.getBoundingClientRect();
          return box.width > 0 && box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom;
        }).map((node) => node.dataset.id ?? "");
      });
      await browser.waitUntil(async () => (await visible()).includes(name(124)),
        { timeout: 30000, timeoutMsg: "the chain's last Artifact is visible after the initial fit" });
      // Zoom in until the chain extends beyond the canvas and its last Artifact is out of view.
      const zoomIn = await browser.$(ZOOM_IN);
      for (let step = 0; step < 12; step += 1) await zoomIn.click();
      await browser.waitUntil(async () => !(await visible()).includes(name(124)),
        { timeout: 15000, timeoutMsg: "the chain's end lies beyond the canvas" });
      // Fit View moves the camera back over the whole chain: the Artifacts it brings into view
      // are drawn, the chain's end among them.
      await fitButton(browser).click();
      await browser.waitUntil(async () => (await visible()).includes(name(124)), { timeout: 15000, timeoutMsg: "the camera drew the chain's last Artifact in view" });
      // The Artifact brought into view takes the pointer like any other.
      await graphShown(browser, name(124));
      await openArtifact(browser, name(124), "Artifact n124");
    },
  ),

  // UX journey: J-DETAIL
  scenario<{ run: string }>(
    "graph-eval-tabs",
    "lists an Artifact's evals on its Artifact tab, shows one's Summary from the list with its Debug facts and its Runtime log, returns to the Artifact tab by its chip, and shows another eval from the tabs",
    async () => {
      return ended(createEvals(["alpha", "beta", "gamma"], "lib"));
    },
    async (browser, { fixture, data: { run } }) => {
      await shownRun(browser, run);
      await graphShown(browser, "a:app");
      const opened = await openArtifact(browser, "a:app", "Artifact app");
      assert.deepEqual(opened.tabs, [], "app's own tab has no chips");
      assert.deepEqual(opened.evals.map((row) => row.name), ["alpha", "beta", "gamma"], "the Artifact tab lists app's own evals");
      assert.ok(opened.evals.every((row) => /GREEN/.test(row.status)), "with their verdicts");
      // A click on an eval of the list shows its tab: its chips, app's own then one per eval,
      // and its own status, instruction and result.
      await openTab(browser, "e:app/beta", { row: true });
      const tabbed = (await detail(browser))!;
      assert.deepEqual(tabbed.tabs.map((tab) => tab.name), ["app", "alpha", "beta", "gamma"], "app's own chip, then one per eval");
      assert.ok(tabbed.tabs.slice(1).every((tab) => tab.glyph === "green"), "each eval's chip shows its GREEN mark");
      const request = requests(fixture).find((saved) => saved.evalId === "app/beta");
      assert.ok(request?.result, "the real CLI saved app/beta's result");
      await browser.waitUntil(async () => (await summary(browser)).instruction.includes("Check beta."), { timeout: 15000, timeoutMsg: "the Detail is app/beta's" });
      const shown = await summary(browser);
      assert.match(shown.status, /GREEN/, "its status");
      assert.ok(shown.blocks.includes("Result"), `its result: ${shown.blocks}`);
      assert.equal((await detail(browser))?.evals.length, 0, "an eval's tab does not list the Artifact's evals");
      // Every other fact folds under Debug, which t shows and hides.
      assert.equal(shown.debug, false, "Debug starts folded");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await fact(browser, "Request")) === request.id, "app/beta's Debug facts shown");
      assert.deepEqual(JSON.parse(await fact(browser, "Raw result")), request.result, "its exact saved result");
      await press(browser, ["t"], async () => (await summary(browser)).debug === false && (await fact(browser, "Request")) === "", "its Debug facts hidden");
      // Below the tabs, the eval switches between its Summary, shown first, and its Runtime log.
      const views = () => browser.execute(() => [...document.querySelectorAll('aside[aria-label="Detail"] [role="tablist"][aria-label="Views"] [role="tab"]')]
        .map((view) => [view.textContent?.trim() ?? "", view.getAttribute("aria-selected")]));
      assert.deepEqual(await views(), [["Summary", "true"], ["Runtime log", "false"]]);
      /** The shown Runtime log, as its box is named and what it holds; "" while none is shown. */
      const log = () => browser.execute(() => { const pre = document.querySelector('aside[aria-label="Detail"] .evidence pre'); return pre ? `${pre.getAttribute("aria-label") ?? ""} ${pre.textContent ?? ""}` : ""; });
      const view = async (at: number) => (await aside(browser).$$('[aria-label="Views"] [role="tab"]'))[at];
      await (await view(1)).click();
      await browser.waitUntil(async () => /Saved runtime stdout \/ stderr/.test(await log()) && (await summary(browser)).instruction === "",
        { timeout: 15000, timeoutMsg: "app/beta's Runtime log shown instead of its Summary" });
      assert.deepEqual(await views(), [["Summary", "false"], ["Runtime log", "true"]]);
      await (await view(0)).click();
      await browser.waitUntil(async () => (await summary(browser)).instruction.includes("Check beta.") && (await log()) === "",
        { timeout: 15000, timeoutMsg: "app/beta's Summary shown again" });
      // From app/beta's Runtime log, app's own chip shows its Artifact tab again.
      await (await view(1)).click();
      await browser.waitUntil(async () => /Saved runtime stdout \/ stderr/.test(await log()), { timeout: 15000, timeoutMsg: "app/beta's Runtime log shown again" });
      const back = await openArtifactTab(browser, "app");
      assert.deepEqual(back.evals.map((row) => row.name), ["alpha", "beta", "gamma"], "the Artifact tab lists app's evals again");
      assert.deepEqual(back.tabs, [], "app's own tab, without chips");
      assert.equal(await log(), "", "app/beta's log is gone");
      // Another eval's tab shows that eval.
      await openTab(browser, "e:app/gamma");
      await browser.waitUntil(async () => (await summary(browser)).instruction.includes("Check gamma."), { timeout: 15000, timeoutMsg: "the Detail is app/gamma's" });
      assert.equal(await browser.$(tabAt("e:app/gamma")).getAttribute("aria-selected"), "true");
      // With its Runtime log shown, a click on another Artifact of the graph shows that
      // Artifact's own tab instead; Esc closes the Detail.
      await (await view(1)).click();
      await browser.waitUntil(async () => /Saved runtime stdout \/ stderr/.test(await log()), { timeout: 15000, timeoutMsg: "app/gamma's Runtime log shown" });
      const other = await openArtifact(browser, "a:lib", "Artifact lib");
      assert.deepEqual(other.evals.map((row) => row.name), ["ready"], "lib's own eval, none of app's");
      assert.equal(await log(), "", "app/gamma's log is gone");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
    },
  ),

  scenario<Record<string, never>>(
    "graph-detail-neighbours",
    "draws an Artifact's Detail between the Artifacts its evals use and those using it, then its evals, path and Debug, and opens a neighbour's Detail on a click",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      const geometry = (selector: string) => browser.execute((selector) => {
        const root = document.querySelector(selector)!;
        return { transform: root.querySelector(".svelte-flow__viewport")?.getAttribute("style"),
          nodes: [...root.querySelectorAll(".svelte-flow__node")].map((node) => [node.getAttribute("data-id"), node.getAttribute("style")]) };
      }, selector);
      const move = async (point: { x: number; y: number }) => {
        await dispatchInput(browser, { kind: "pointer", phase: "down", ...point });
        await afterRender(browser);
        const end = { x: point.x + 40, y: point.y - 15 };
        await dispatchInput(browser, { kind: "pointer", phase: "move", ...end });
        await dispatchInput(browser, { kind: "pointer", phase: "up", ...end });
        await afterRender(browser);
      };
      const background = (selector: string) => browser.execute((selector) => {
        const box = document.querySelector(selector)!.getBoundingClientRect();
        return { x: Math.round(box.left + box.width / 2), y: Math.round(box.bottom - 3) };
      }, selector);
      const main = await geometry(".graph");
      await move(await background(".graph"));
      assert.notDeepEqual(await geometry(".graph"), main, "the same geometry probe observes a real main-graph pan");
      // Each node drawn in the Detail's neighbours, left to right: the Artifacts its evals use,
      // the Artifact itself, then those whose evals use it.
      const neighbours = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('aside[aria-label="Detail"] [role="group"][aria-label="Neighbours"] .svelte-flow__node')]
        .map((node) => ({ id: node.getAttribute("data-id") ?? "", x: node.getBoundingClientRect().left }))
        .sort((a, b) => a.x - b.x).map((node) => node.id));
      await openArtifact(browser, "a:release", "Artifact release");
      // Neighbours is folded until unfolded; unfolded, it stays so for the Details opened later.
      assert.equal(await aside(browser).$('[role="group"][aria-label="Neighbours"]').isExisting(), false, "Neighbours starts folded");
      await unfold(browser, "Neighbours");
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "release drawn before ship, which uses it" });
      const drawing = '[aria-label="Detail"] [role="group"][aria-label="Neighbours"]';
      let edges: (string | null)[] = [];
      await browser.waitUntil(async () => {
        // Only an edge whose line is actually painted counts as drawn.
        edges = await browser.execute((selector) => [...document.querySelectorAll(`${selector} .svelte-flow__edge`)].filter((edge) => {
          const path = edge.querySelector("path");
          if (!path) return false;
          const box = path.getBoundingClientRect();
          const style = getComputedStyle(path);
          return (box.width > 0 || box.height > 0) && style.visibility === "visible" && style.display !== "none"
            && Number(style.opacity) > 0 && style.stroke !== "none" && Number.parseFloat(style.strokeWidth) > 0;
        }).map((edge) => edge.getAttribute("data-id")), drawing);
        return JSON.stringify(edges) === JSON.stringify(["a:release>a:ship"]);
      }, { timeout: 15000, timeoutMsg: "the neighbour drawing renders its actual input-to-consumer relation" });
      assert.deepEqual(edges, ["a:release>a:ship"]);
      const fixed = await geometry(drawing);
      const point = await background(drawing);
      await move(point);
      assert.deepEqual(await geometry(drawing), fixed, "the neighbour drawing does not pan");
      await browser.action("wheel").scroll({ ...point, deltaX: 0, deltaY: -120 }).perform();
      await afterRender(browser);
      assert.deepEqual(await geometry(drawing), fixed, "the neighbour drawing does not zoom");
      await move(await visibleCenter(browser, await aside(browser).$('[aria-label="Neighbours"] .svelte-flow__node[data-id="a:release"]').getElement()));
      assert.deepEqual(await geometry(drawing), fixed, "its nodes do not drag");
      // Its evals with their verdicts, the drawing, then its path; every other fact folds under
      // Debug, unfolded with t.
      const path = () => browser.execute(() =>
        document.querySelector('aside[aria-label="Detail"] section[aria-label="Path"] p.line')?.textContent?.trim() ?? "");
      await browser.waitUntil(async () => (await path()) === "release", { timeout: 15000, timeoutMsg: "release's path below its neighbours" });
      assert.deepEqual((await detail(browser))?.evals.map((row) => row.name), ["signoff"], "release's one eval listed");
      assert.equal((await summary(browser)).debug, false, "the Artifact's Debug starts folded");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await summary(browser)).debug === true && (await fact(browser, "Tags")) !== "", "the Artifact's Debug unfolded by t");
      const neighbour = (id: string) => aside(browser).$(`[aria-label="Neighbours"] .svelte-flow__node[data-id="${id}"]`);
      await neighbour("a:ship").click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "ship's Detail opened from its neighbour" });
      assert.deepEqual((await detail(browser))?.tabs, [], "on its Artifact tab");
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "ship drawn after release, which it uses" });
      // What ship waits for is a Debug fact, not shown above it.
      assert.equal(await browser.execute(() => [...document.querySelectorAll('aside[aria-label="Detail"] .section > .head')]
        .some((heading) => heading.textContent?.trim() === "Waits for")), false, "ship's waits are not shown outside Debug");
      if (!(await summary(browser)).debug) {
        await aside(browser).$('[role="document"]').click();
        await press(browser, ["t"], async () => (await summary(browser)).debug === true, "ship's Debug unfolded by t");
      }
      assert.match(await fact(browser, "Waits for"), /release/, "ship waits for release, under Debug");
      assert.equal(await path(), "ship", "the Artifact's path is below its neighbour drawing");
      await neighbour("a:release").click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact release", { timeout: 15000, timeoutMsg: "release's Detail again" });
      // A click on an eval listed on the Artifact tab shows that eval's tab.
      await openTab(browser, "e:release/signoff", { row: true });
    },
  ),

  // UX journey: J-DETAIL-NEIGHBOURS
  scenario<{ run: string; address: string }>(
    "graph-detail-live-target",
    "shows the Live target and its address in the saved Run's Artifact Path section",
    async () => {
      const fixture = createOutside();
      const address = "https://example.invalid/e2e/live-application";
      writeText(nativePath(makeDirectory(nativePath(fixture.repo, ".artifactize")), "remote.artf"), `path = ${JSON.stringify(address)}
[evals.check]
title = "Check the Live Artifact"
profile = ${runtimeProfile()}
payload.instruction = "Check the Live Artifact."
`);
      git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
      assert.equal(artifactize(fixture, "verify", "--all", "--wait"), 0);
      return { ...ended(fixture), data: { run: savedRuns(fixture)[0].id, address } };
    },
    async (browser, { data: { run, address } }) => {
      await shownRun(browser, run);
      await graphShown(browser, "a:remote");
      await openArtifact(browser, "a:remote", "Artifact remote");
      const path = aside(browser).$('section[aria-label="Path"]');
      await browser.waitUntil(async () => await path.isDisplayed() && (await path.getText()).includes(address),
        { timeout: 15000, timeoutMsg: "the Live Artifact's Path is shown" });
      const shown = await path.getText();
      assert.match(shown, /Live target/);
      assert.ok(shown.includes(address), "the address in Path comes from the saved Run");
    },
  ),

  // UX journeys: J-GRAPH-FIND J-PANE-SEARCH
  scenario<Record<string, never>>(
    "graph-find",
    "Ctrl+F searches the Pane from the Pane; over the Run's graph it finds Artifacts and evals by name, opens the chosen one's Detail on its tab with the Artifact in the middle of the graph left of the Detail, and Esc or a click outside closes it",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const field = () => findField(browser);
      const names = async () => (await field())?.entries.map((entry) => entry.name) ?? null;
      const zoom = () => browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText();
      /** The named Artifact is centered to the left of the Detail without changing the readable zoom. */
      const centred = async (id: string, before: string) => {
        const at = await offCentre(browser, id);
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && await zoom() === before
          && await browser.$(`${nodeAt(id)} .name`).isDisplayed();
      };
      const inDetail = () => browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]'));
      await graphShown(browser, "a:release");
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });

      // In the Pane, Ctrl+F focuses the Pane's search, as / does; no find field opens.
      await press(browser, [primaryModifier, "f"], () => searchFocused(browser), "Ctrl+F in the Pane focuses its search");
      assert.equal(await field(), null, "no find field over the graph");
      await press(browser, ["Escape"], () => entriesFocused(browser), "Esc on sort:recent back to the entries");
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");

      // The Find button opens a focused field. Closing it neither moves the camera nor opens a Detail.
      const first = await camera(browser);
      const findButton = () => browser.$('[aria-label="Graph tools"]').$("button=Find");
      await findButton().click();
      await browser.waitUntil(async () => (await field())?.note === NO_FAVORITES && (await field())?.focused === true,
        { timeout: 15000, timeoutMsg: "the Find button opens the focused field, saying how to star one" });
      assert.deepEqual(await field(), { focused: true, value: "", entries: [], note: NO_FAVORITES });
      assert.equal(await searchFocused(browser), false, "the Pane's search stays unfocused");
      await fill(browser, FIND, "nothing");
      await browser.waitUntil(async () => (await field())?.note === "No Artifact or eval matches.", { timeout: 15000, timeoutMsg: "nothing found said so" });
      assert.deepEqual(await names(), []);
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");
      assert.equal(await camera(browser), first, "Esc moved no camera");
      assert.equal(await detail(browser), null, "Esc opened nothing");

      // With docs's Detail open, Ctrl+F finds ship; a click on it opens ship's Detail and brings
      // ship to the middle of the graph left of the Detail.
      await openArtifact(browser, "a:docs", "Artifact docs");
      // At natural size or larger the names already read. Use the displayed zoom, not a node width.
      for (let step = 0; step < 12 && Number.parseInt(await zoom(), 10) < 100; step += 1) {
        await browser.$(ZOOM_IN).click();
        await afterRender(browser);
      }
      assert.ok(Number.parseInt(await zoom(), 10) >= 100, "the graph is at an already readable zoom");
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open over docs's Detail");
      await fill(browser, FIND, "ship");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(["ship"]), { timeout: 15000, timeoutMsg: "ship found" });
      assert.ok((await field())?.entries.every((entry) => !entry.chosen), "no row is chosen when results first appear");
      const shipBefore = await zoom();
      await browser.$('[aria-label="Found"] [role="option"]').click();
      await browser.waitUntil(async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.title === "Artifact ship" && shown.tabs.length === 0 && shown.evals.length > 0;
      }, { timeout: 15000, timeoutMsg: "ship's Detail on its Artifact tab, the field closed" });
      await browser.waitUntil(() => centred("a:ship", shipBefore), { timeout: 15000, timeoutMsg: "ship in the middle of the graph left of the Detail" });
      assert.ok(await inDetail(), "the keyboard is in the Detail");

      // A click outside the field, on the Detail's heading, closes it: the Detail and the camera
      // stay as they were.
      const shipCamera = await camera(browser);
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open again");
      await fill(browser, FIND, "s");
      await browser.waitUntil(async () => ((await names())?.length ?? 0) > 0, { timeout: 15000, timeoutMsg: "names with s listed" });
      // A press on its text, as a pointer gives it: focus moves to the Detail.
      await dispatchInput(browser, { kind: "text-click", ...await visibleCenter(browser, await aside(browser).$("h2").getElement()) });
      await browser.waitUntil(async () => (await field()) === null, { timeout: 15000, timeoutMsg: "the click outside closed the field" });
      assert.equal((await detail(browser))?.title, "Artifact ship", "the Detail stays");
      assert.equal(await camera(browser), shipCamera, "the click outside moved no camera");

      // Typing lists what contains the text, ignoring case: release's eval signoff, with its
      // verdict's glyph and colour. ↓ chooses it; Enter opens release's Detail on that eval's tab.
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open from the Detail");
      await fill(browser, FIND, "SIGN");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(["release / signoff"]), { timeout: 15000, timeoutMsg: "release's signoff found" });
      const [found] = (await field())!.entries;
      assert.equal(found.chosen, false, "nothing chosen before ↓");
      await press(browser, ["ArrowDown"], async () => (await field())?.entries[0]?.chosen === true, "↓ chooses the eval");
      const releaseBefore = await zoom();
      await press(browser, ["Enter"], async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.tabs[0]?.name === "release" && shown.tabs.find((each) => each.selected)?.name === "signoff";
      }, "Enter opens release's Detail on its signoff tab");
      const tab = (await detail(browser))!.tabs.find((each) => each.selected)!;
      // The fixture's signoff waits for a person: its known mark is the human one, in the
      // colour its tab paints that mark.
      assert.equal(found.glyph, "human", "the found eval showed the waiting Human mark");
      assert.deepEqual([found.glyph, found.colour], [tab.glyph, tab.colour], "the found eval showed its verdict's glyph and colour");
      await browser.waitUntil(() => centred("a:release", releaseBefore), { timeout: 15000, timeoutMsg: "release in the middle of the graph left of the Detail" });
      assert.ok(await inDetail(), "the keyboard is in the Detail");

      // A click on a found eval does the same: Find closes, release's Detail shows the eval's
      // tab, and release is centred.
      await openArtifact(browser, "a:ship", "Artifact ship");
      await browser.$('[aria-label="Graph tools"]').$("button=Find").click();
      await browser.waitUntil(async () => (await field())?.focused === true, { timeout: 15000, timeoutMsg: "the Find button opened the field" });
      await fill(browser, FIND, "SIGN");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(["release / signoff"]), { timeout: 15000, timeoutMsg: "release's signoff found again" });
      const clickedBefore = await zoom();
      await browser.$('.find [role="option"]').click();
      await browser.waitUntil(async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.tabs[0]?.name === "release" && shown.tabs.find((each) => each.selected)?.name === "signoff";
      }, { timeout: 15000, timeoutMsg: "a click on the eval opened release's Detail on its signoff tab" });
      await browser.waitUntil(() => centred("a:release", clickedBefore), { timeout: 15000, timeoutMsg: "release centred after the click" });

      // A text field outside the Pane is not graph focus, even with a Run graph shown.
      const human = aside(browser).$('[aria-label="Human review"]');
      await human.$("button=Claim to review").click();
      await human.$("button=GREEN").click();
      const comment = human.$("#field-comment");
      await comment.click();
      await browser.waitUntil(() => comment.isFocused(), { timeout: 15000, timeoutMsg: "the Human result text field has focus" });
      await press(browser, [primaryModifier, "f"], () => searchFocused(browser), "Ctrl+F in the result field focuses pane Search");
      assert.equal(await field(), null, "no graph Find opens from the focused result field");
      await human.$("button=Release claim").click();
      await human.$("button=Claim to review").waitForDisplayed({ timeout: 15000, timeoutMsg: "the review claim is released again" });
    },
  ),

  // UX journeys: J-DETAIL-STAR J-GRAPH-FIND
  scenario<{ request: string; other: string }>(
    "graph-favorites",
    "stars Artifacts with the ☆ beside their Detail's title on any tab; Ctrl+F with nothing typed lists the starred ones in the graph's order and opens one as found, on the Run and the Current alike, says how to star one while none is; the stars stay for the next monitor and for every branch of the project",
    async () => {
      const fixture = await create();
      // master's sign-off, taken before the other worktrees add theirs.
      const request = signoff(fixture).id;
      const [other] = await addWorktrees(fixture);
      return { fixture, args: ["--repo", fixture.repo], data: { request, other } };
    },
    async (browser, { fixture, data: { request } }) => {
      const field = () => findField(browser);
      const names = async () => (await field())?.entries.map((entry) => entry.name) ?? null;
      const ctrlF = (what: string) => press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, what);
      await graphShown(browser, "a:release");
      // No hint at the graph's bottom: the tool strip's Find, the palette and Help name its keys.
      assert.equal(await hint(browser), "");
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");

      // None starred: the tool strip's Find opens the field, empty and with the keyboard, over the
      // graph's top left, saying how to star one; so does Ctrl+F.
      await browser.$('[aria-label="Graph tools"]').$("button=Find").click();
      await browser.waitUntil(async () => (await field())?.focused === true && (await field())?.note === NO_FAVORITES,
        { timeout: 15000, timeoutMsg: "Find opened the field, saying how to star one" });
      assert.deepEqual(await names(), []);
      assert.equal((await field())?.value, "", "it opens empty");
      const placedAt = await browser.execute((find) => {
        const input = document.querySelector(find)!.getBoundingClientRect();
        const canvas = document.querySelector(".graph .canvas")!.getBoundingClientRect();
        return { left: input.left - canvas.left, top: input.top - canvas.top, width: canvas.width };
      }, FIND);
      assert.ok(placedAt.left >= 0 && placedAt.top >= 0 && placedAt.left < placedAt.width / 2 && placedAt.top < 80, `over the graph's top left: ${JSON.stringify(placedAt)}`);
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");
      await ctrlF("the find field open with nothing starred");
      await browser.waitUntil(async () => (await field())?.note === NO_FAVORITES, { timeout: 15000, timeoutMsg: "how to star one said" });
      assert.deepEqual(await names(), []);
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");

      // ship starred on its Artifact tab; release on its signoff tab, a Human review's; docs
      // starred and its star taken away again.
      await openArtifact(browser, "a:ship", "Artifact ship");
      assert.deepEqual(await star(browser, "ship"), { glyph: "☆", on: false }, "ship not starred yet");
      await toggleStar(browser, "ship", true);
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await toggleStar(browser, "release", true);
      await openArtifact(browser, "a:docs", "Artifact docs");
      await toggleStar(browser, "docs", true);
      await toggleStar(browser, "docs", false);
      // In the fixture's graph, ship reads release, so release precedes ship. They
      // were starred in the opposite order; searching does not supply the oracle.
      const favorites = ["release", "ship"];
      await ctrlF("the find field open over docs's Detail");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(favorites),
        { timeout: 15000, timeoutMsg: "favorites listed in their declared graph order" });
      await fill(browser, FIND, "s");
      await browser.waitUntil(async () => (await names())?.includes("docs") === true, { timeout: 15000, timeoutMsg: "every Artifact with s found" });
      // The text removed again lists the favorites, in that order, each with its glyph.
      await fill(browser, FIND, "");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(favorites), { timeout: 15000, timeoutMsg: `the favorites listed: ${JSON.stringify(await field())}` });
      assert.equal(await browser.$('[aria-label="Favorites"]').isExisting(), true, "listed as the favorites");
      assert.ok((await field())!.entries.every((entry) => entry.glyph !== ""), "each with its verdict's glyph");
      // ↓ twice chooses the second; Enter opens it as a found Artifact opens.
      await press(browser, ["ArrowDown"], async () => (await field())?.entries[0]?.chosen === true, "↓ chooses the first");
      await press(browser, ["ArrowDown"], async () => (await field())?.entries[1]?.chosen === true, "↓ chooses the second");
      await press(browser, ["Enter"], async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.title === `Artifact ${favorites[1]}` && shown.tabs.length === 0 && shown.evals.length > 0;
      }, `Enter opens ${favorites[1]}'s Detail on its Artifact tab`);
      assert.deepEqual(await star(browser, favorites[1]), { glyph: "★", on: true }, "its star on");
      assert.ok(await browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]')), "the keyboard is in the Detail");

      // The branch's Current, once its Run ended, has the same stars: its Detail shows ship
      // starred, and Ctrl+F lists the favorites; a click on one opens it.
      signOff(fixture, request);
      await chooseChip(browser, "repo  master", "current");
      await browser.waitUntil(async () => (await mainTitle(browser)).startsWith("Current"), { timeout: 15000, timeoutMsg: "the Current shown" });
      await graphShown(browser, "a:release");
      assert.equal(await hint(browser), "");
      await openArtifact(browser, "a:ship", "Artifact ship");
      assert.deepEqual(await star(browser, "ship"), { glyph: "★", on: true }, "ship starred on the Current");
      await ctrlF("the find field open over the Current");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(favorites), { timeout: 15000, timeoutMsg: "the Current's favorites listed" });
      await browser.$('[aria-label="Favorites"] [role="option"]').click();
      await browser.waitUntil(async () => (await field()) === null && (await detail(browser))?.title === `Artifact ${favorites[0]}`, { timeout: 15000, timeoutMsg: `${favorites[0]}'s Detail opened from the favorites` });
    },
    // The next monitor keeps the stars, for every branch of the project: another worktree's Run
    // shows them and lists them; a star taken away there is gone from master's Current too.
    async (browser, { data: { other } }) => {
      const field = () => findField(browser);
      const listedNames = async () => [...(await field())?.entries.map((entry) => entry.name) ?? []].sort();
      // master's Run ended: the launch shows its Current.
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:ship", "Artifact ship");
      assert.deepEqual(await star(browser, "ship"), { glyph: "★", on: true }, "ship's star kept");

      await listed(browser, "repo  scope-one");
      await showRun(browser, "repo  scope-one", other);
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:ship", "Artifact ship");
      assert.deepEqual(await star(browser, "ship"), { glyph: "★", on: true }, "ship starred on scope-one's Run");
      await press(browser, [primaryModifier, "f"], async () => JSON.stringify(await listedNames()) === JSON.stringify(["release", "ship"]), "scope-one's Run lists both favorites");
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");
      await toggleStar(browser, "ship", false);

      await chooseChip(browser, "repo  master", "current");
      await browser.waitUntil(async () => (await mainTitle(browser)).startsWith("Current"), { timeout: 15000, timeoutMsg: "master's Current shown" });
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:ship", "Artifact ship");
      assert.deepEqual(await star(browser, "ship"), { glyph: "☆", on: false }, "ship no longer starred on master's Current");
      await press(browser, [primaryModifier, "f"], async () => JSON.stringify(await listedNames()) === JSON.stringify(["release"]), "only release listed on master's Current");
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");
    },
  ),

  // UX journey: J-GRAPH-FIND
  scenario<Record<string, never>>(
    "graph-find-many",
    "lists the first 50 Artifacts of a Current that match, in the graph's order, says how many more do, and Enter opens the first",
    async () => {
      // Its Runs ended, so the branch's newest entry, shown at launch, is its Current.
      const fixture = createChain(53);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const field = () => findField(browser);
      await browser.$('section[aria-label="Current"] .svelte-flow__node[data-id="a:n052"]').waitForExist({ timeout: 15000, timeoutMsg: "the Current's graph drawn" });
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });
      // Once scanned, every Artifact of the unchanged chain would be reused.
      await browser.waitUntil(async () => /^Scanned at \d/.test(await scanned(browser)), { timeout: 30000, timeoutMsg: "the Current scanned" });
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      const zoom = async () => Number.parseInt(await browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText(), 10);
      const before = await zoom();
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open over the Current");
      await fill(browser, FIND, "N0");
      await browser.waitUntil(async () => (await field())?.entries.length === 50, { timeout: 15000, timeoutMsg: "fifty found" });
      const shown = (await field())!;
      const expected = Array.from({ length: 50 }, (_, index) => `n${String(index).padStart(3, "0")}`);
      assert.deepEqual(shown.entries.map((entry) => entry.name), expected, "the first fifty in the graph's order, upstream first");
      assert.equal(shown.note, "3 more match.");
      // Enter with nothing chosen opens the first: n000, at the far left of the chain.
      await press(browser, ["Enter"], async () => (await field()) === null && (await detail(browser))?.title === "Artifact n000", "Enter opens n000's Detail");
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, "a:n000");
        const name = browser.$(`${nodeAt("a:n000")} .name`);
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && await zoom() > before
          && await name.isDisplayed() && await name.getText() === "n000";
      }, { timeout: 15000, timeoutMsg: "n000 centered left of the Detail and zoomed in to show its name" });
      // Each listed Artifact showed what verify would do with it, reused, as its eval's row in
      // the Detail of the Current shows it.
      const reused = await browser.execute(() => {
        const glyph = document.querySelector('aside[aria-label="Detail"] button[data-eval] .glyph');
        return { glyph: glyph?.querySelector("svg")?.getAttribute("data-icon") ?? "", colour: glyph ? getComputedStyle(glyph).color : "" };
      });
      assert.equal(reused.glyph, "reused", "the Detail's eval row says reused");
      assert.deepEqual(shown.entries.filter((entry) => entry.glyph !== reused.glyph || entry.colour !== reused.colour), [],
        "every found Artifact showed reused, in its glyph and colour");

      // ↓ and ↑ move the choice one entry at a time; Enter opens the chosen one, n001.
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open again");
      await fill(browser, FIND, "N0");
      await browser.waitUntil(async () => (await field())?.entries.length === 50, { timeout: 15000, timeoutMsg: "fifty found again" });
      const chosen = async () => (await field())?.entries.findIndex((entry) => entry.chosen);
      for (const [key, at] of [["ArrowDown", 0], ["ArrowDown", 1], ["ArrowDown", 2], ["ArrowUp", 1]] as const) {
        await press(browser, [key], async () => (await chosen()) === at, `${key} chooses entry ${at}`);
      }
      await press(browser, ["Enter"], async () => (await field()) === null && (await detail(browser))?.title === "Artifact n001", "Enter opens the chosen n001's Detail");
    },
  ),

  // UX journey: J-GRAPH-FIND
  scenario<{ run: string }>(
    "graph-find-order",
    "lists matching names in the shown Run's dependency order rather than alphabetical order",
    async () => ended(createChain(3, false, (index) => ["stage-zulu", "stage-alpha", "stage-mike"][index])),
    async (browser, { data: { run } }) => {
      await shownRun(browser, run);
      await graphShown(browser, "a:stage-zulu");
      await browser.$('[aria-label="Graph tools"]').$("button=Find").click();
      await fill(browser, FIND, "STAGE-");
      const expected = ["stage-zulu", "stage-alpha", "stage-mike"];
      const matches = async () => (await findField(browser))?.entries.map(({ name }) => name);
      await browser.waitUntil(async () => (await matches())?.length === expected.length,
        { timeout: 15000, timeoutMsg: "all three stage Artifacts were found" });
      assert.deepEqual(await matches(), expected, "the prerequisite stage comes before both its consumers despite its name");
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === "Artifact stage-zulu",
        "the first match opens the upstream stage");
    },
  ),

  // UX journey: J-GRAPH-MINIMIZE
  scenario<{ run: string }>(
    "graph-minimize-joined",
    "draws the edges of a minimized whole's parts to the same Artifact as one, lit by the light that comes first",
    async () => {
      const fixture = createJoined();
      const [run] = savedRuns(fixture);
      return { fixture, args: ["--repo", fixture.repo], data: { run: run.id } };
    },
    async (browser, { data }) => {
      await shownRun(browser, data.run);
      await graphShown(browser, "a:pack/a:bad");
      // Before: good's GREEN edge and bad's RED edge both reach user.
      await browser.waitUntil(async () => graphHasColours(await graphAppearance(browser), { "a:pack/a:good": "green", "a:pack/a:bad": "red" }, {
        "a:pack/a:good>a:user": "green",
        "a:pack/a:bad>a:user": "red",
      }), { timeout: 15000, timeoutMsg: "good's and bad's edges to user drawn in their lights" });
      // Minimized, pack sends user one edge, RED before GREEN.
      await clickButton(browser, ".graph", "Minimize pack (m)");
      let edges: string[] = [];
      await browser.waitUntil(async () => {
        const shown = await graphAppearance(browser);
        edges = Object.keys(shown.edges);
        return graphHasColours(shown, {}, { "a:pack>a:user": "red" });
      }, { timeout: 15000, timeoutMsg: "pack's one edge to user drawn RED" });
      assert.deepEqual(edges, ["a:pack>a:user"], "the two edges drawn as one");
    },
  ),
  // UX journey: J-GRAPH-NAVIGATOR
  scenario<{ run: string }>(
    "graph-navigator",
    "shows the Navigator once part of the graph lies outside the view, and moves the camera to the place clicked or dragged to in it, the zoom kept",
    async () => ended(createChain(30, true)),
    async (browser, { data: { run } }) => {
      await shownRun(browser, run);
      await graphShown(browser, "a:n000");
      /** The Navigator's box and the frame of the part in view, on the screen; null while it is not shown. */
      const navigator = () => browser.execute(() => {
        const map = document.querySelector("[data-navigator] .map")?.getBoundingClientRect();
        const frame = document.querySelector("[data-navigator] .frame")?.getBoundingClientRect();
        return map && frame ? { map: map.toJSON() as DOMRect, frame: frame.toJSON() as DOMRect } : null;
      });
      const zoom = () => browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText();
      // First shown whole: no Navigator.
      await camera(browser);
      assert.equal(await navigator(), null, "no Navigator while the whole graph is in view");
      // Zoomed in until part of the chain lies outside the view, the Navigator shows.
      for (let step = 0; step < 12 && (await navigator()) === null; step += 1) {
        await browser.$(ZOOM_IN).click();
        await camera(browser);
      }
      // Zoomed in further until the part in view is a small part of the whole.
      for (let step = 0; step < 12 && ((await navigator())?.frame.width ?? 0) >= ((await navigator())?.map.width ?? 0) * 0.3; step += 1) {
        await browser.$(ZOOM_IN).click();
        await camera(browser);
      }
      const shown = await navigator();
      assert.ok(shown && shown.frame.width < shown.map.width * 0.3 && shown.map.height >= 16,
        `the Navigator, tall enough to point at, frames the part in view: ${JSON.stringify(shown)}`);
      // The whole chain in small, every Artifact a block of its status's colour,
      // at the graph's bottom right.
      const drawing = async () => {
        const { image, blocks, red, green, ...placement } = await browser.execute(() => {
          const navigator = document.querySelector("[data-navigator]")!.getBoundingClientRect();
          const canvas = [...document.querySelectorAll(".canvas")].find((element) => !element.closest('aside[aria-label="Detail"]'))!.getBoundingClientRect();
          const detail = document.querySelector('aside[aria-label="Detail"]')?.getBoundingClientRect() ?? null;
          const map = document.querySelector<HTMLCanvasElement>("[data-navigator] canvas")!;
          const style = getComputedStyle(document.documentElement);
          const colour = (token: string) => [1, 3, 5].map((at) => Number.parseInt(style.getPropertyValue(token).trim().slice(at, at + 2), 16));
          const red = colour("--red"), green = colour("--green");
          // Project each Artifact's center between the graph's own outer edges onto the painting.
          const nodes = [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")]
            .filter((node) => !node.closest('aside[aria-label="Detail"]'))
            .map((node) => ({ id: node.dataset.id!, box: node.getBoundingClientRect() }));
          const left = Math.min(...nodes.map(({ box }) => box.left));
          const right = Math.max(...nodes.map(({ box }) => box.right));
          const top = Math.min(...nodes.map(({ box }) => box.top));
          const bottom = Math.max(...nodes.map(({ box }) => box.bottom));
          const blocks = nodes.map(({ id, box }) => {
            const x = Math.floor((box.left + box.width / 2 - left) / (right - left) * map.width);
            const y = Math.floor((box.top + box.height / 2 - top) / (bottom - top) * map.height);
            return {
              id,
              x, y,
              outside: box.right < canvas.left || box.left > canvas.right || box.bottom < canvas.top || box.top > canvas.bottom,
            };
          });
          return {
            atBottomRight: navigator.left > canvas.left + canvas.width / 2 && navigator.top > canvas.top + canvas.height / 2 &&
              navigator.right <= canvas.right && navigator.bottom <= canvas.bottom,
            leftOfDetail: detail !== null && navigator.right <= detail.left,
            image: map.toDataURL(), blocks, red, green,
          };
        });
        const painting = canvasSnapshot(image);
        return { ...placement, blocks: blocks.map(({ x, y, ...block }) => {
          const rgba = painting.at(x, y);
          const matches = (expected: number[]) => rgba[3] > 0 && expected.every((value, channel) => Math.abs(rgba[channel] - value) <= 8);
          return { ...block, colour: matches(red) ? "red" : matches(green) ? "green" : null };
        }) };
      };
      // The bitmap is read only once it has painted every block in its verdict's colour.
      let look = await drawing();
      await browser.waitUntil(async () => {
        look = await drawing();
        return look.blocks.length === 30 && look.blocks.every(({ colour }) => colour !== null);
      }, { timeout: 15000, timeoutMsg: "the Navigator painted every block" }).catch(() => undefined);
      assert.deepEqual(look.blocks.map(({ id }) => id).sort(), Array.from({ length: 30 }, (_, n) => `a:n${String(n).padStart(3, "0")}`));
      assert.ok(look.blocks.some(({ outside }) => outside) && look.blocks.every(({ id, colour }) => colour === (id === "a:n029" ? "red" : "green")),
        `every Artifact has a block in its own verdict colour, including those fully outside the view: ${JSON.stringify(look.blocks)}`);
      assert.ok(look.atBottomRight, "the Navigator is at the graph's bottom right");
      const unhovered = await camera(browser);
      const mapPoint = { x: Math.round(shown.map.left + shown.map.width / 2), y: Math.round(shown.map.top + shown.map.height / 2) };
      await browser.action("pointer").move({ ...mapPoint, origin: "viewport" }).perform();
      await afterRender(browser);
      assert.equal(await detail(browser), null, "hovering the Navigator opens no Detail");
      assert.equal(await browser.$('[role="tooltip"]').isDisplayed(), false, "hovering the Navigator opens no tooltip");
      assert.equal(await camera(browser), unhovered, "hovering leaves the camera in place");
      const kept = await zoom();
      /** The frame's centre, across the Navigator. */
      const centre = async () => { const at = (await navigator())!; return at.frame.left + at.frame.width / 2; };
      const pointAt = (x: number, y: number) => ({
        x: Math.round(shown.map.left + shown.map.width * x), y: Math.round(shown.map.top + shown.map.height * y),
      });
      /** Project the chosen point from the drawn mini-map into the graph on both axes. */
      const offset = (point: { x: number; y: number }) => browser.execute(({ x, y }) => {
        const canvas = [...document.querySelectorAll(".canvas")].find((element) => !element.closest('aside[aria-label="Detail"]'))!.getBoundingClientRect();
        const map = document.querySelector("[data-navigator] canvas")!.getBoundingClientRect();
        const nodes = [...document.querySelectorAll(".svelte-flow__node")].filter((node) => !node.closest('aside[aria-label="Detail"]')).map((node) => node.getBoundingClientRect());
        const [left, right] = [Math.min(...nodes.map((box) => box.left)), Math.max(...nodes.map((box) => box.right))];
        const [top, bottom] = [Math.min(...nodes.map((box) => box.top)), Math.max(...nodes.map((box) => box.bottom))];
        const fraction = (at: number, start: number, span: number) => Math.max(0, Math.min(1, (at - start) / span));
        return { x: left + fraction(x, map.left, map.width) * (right - left) - (canvas.left + canvas.right) / 2,
          y: top + fraction(y, map.top, map.height) * (bottom - top) - (canvas.top + canvas.bottom) / 2 };
      }, point);
      const centered = async (point: { x: number; y: number }, action: string) => {
        await browser.waitUntil(async () => {
          const at = await offset(point);
          return Math.abs((await centre()) - point.x) <= 3 && Math.abs(at.x) <= 12 && Math.abs(at.y) <= 4;
        }, { timeout: 15000, timeoutMsg: `${action} centered the off-axis point on both axes` })
          .catch(async (error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(await offset(point))}`, { cause: error }); });
        assert.equal(await zoom(), kept, `${action} kept the zoom`);
      };
      const before = await camera(browser);
      const there = pointAt(0.35, 0.25);
      await dispatchInput(browser, { kind: "pointer", phase: "down", ...there });
      await dispatchInput(browser, { kind: "pointer", phase: "up", ...there });
      await centered(there, "the click");
      assert.notEqual(await camera(browser), before, "the click moved the camera");
      const further = pointAt(0.65, 0.75);
      await dispatchInput(browser, { kind: "pointer", phase: "down", ...there });
      await afterRender(browser);
      await dispatchInput(browser, { kind: "pointer", phase: "move", ...further });
      await dispatchInput(browser, { kind: "pointer", phase: "up", ...further });
      await centered(further, "the drag");
      // With a Detail open, it sits left of the Detail.
      const inView = await browser.execute(() => {
        const canvas = [...document.querySelectorAll(".canvas")].find((element) => !element.closest('aside[aria-label="Detail"]'))!.getBoundingClientRect();
        return [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].find((node) => {
          const box = node.getBoundingClientRect();
          return box.left >= canvas.left && box.right <= canvas.left + canvas.width / 2 && box.top >= canvas.top && box.bottom <= canvas.bottom;
        })?.dataset.id ?? null;
      });
      assert.ok(inView, "an Artifact in view to open");
      await openArtifact(browser, inView, `Artifact ${inView.slice(2)}`);
      await browser.waitUntil(async () => (await drawing()).leftOfDetail,
        { timeout: 15000, timeoutMsg: `the Navigator left of the Detail: ${JSON.stringify(await drawing())}` });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // Fit shows the whole graph again: the Navigator goes.
      await fitButton(browser).click();
      await browser.waitUntil(async () => (await navigator()) === null, { timeout: 15000, timeoutMsg: "no Navigator once the whole graph is in view" });
    },
  ),

  // UX journey: J-DETAIL-SECTIONS
  scenario<Record<string, never>>(
    "graph-detail-sections",
    "folds and unfolds a Detail's sections by their header rows, on an Artifact's tab, an eval's Summary and a Run's Detail, kept by name for the Details opened later and the next launch",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      /** Each section of the open Detail by its header row, and whether it is unfolded. */
      const sections = () => detailSections(browser);
      const toggle = async (name: string, unfolded: boolean) => {
        await aside(browser).$(`button.head=${name}`).click();
        await browser.waitUntil(async () => (await sections())[name] === unfolded, { timeout: 15000, timeoutMsg: `${name} ${unfolded ? "unfolded" : "folded"}` });
      };
      await graphShown(browser, "a:release");
      // An Artifact's tab: what it is opened for unfolded, what is read now and then folded.
      await openArtifact(browser, "a:release", "Artifact release");
      // Neighbours shows once release's neighbours are read: it has some.
      await browser.waitUntil(async () => (await sections()).Path === true && "Neighbours" in (await sections()), { timeout: 15000, timeoutMsg: "release's sections drawn" });
      const first = await sections();
      assert.deepEqual([first.Evals, first.Neighbours, first.Tools, first.Path, first.Debug], [true, false, true, true, false], JSON.stringify(first));
      const pathHeader = aside(browser).$("button.head=Path");
      await toggle("Path", false);
      await browser.waitUntil(() => pathHeader.isFocused(), { timeout: 15000, timeoutMsg: "the Path header has keyboard focus" });
      await buttonKey(browser, "Enter");
      await browser.waitUntil(async () => (await sections()).Path === true, { timeout: 15000, timeoutMsg: "Enter unfolds Path" });
      assert.ok(await pathHeader.isFocused(), "the unfolded section keeps keyboard focus on its header");
      await buttonKey(browser, "Space");
      await browser.waitUntil(async () => (await sections()).Path === false, { timeout: 15000, timeoutMsg: "Space folds Path" });
      await toggle("Neighbours", true);
      // Folded to its header row alone.
      assert.equal(await aside(browser).$('section[aria-label="Path"] p.line').isDisplayed(), false, "a folded section shows its row alone");
      // Another Artifact's Detail keeps each section as it was left, by its name.
      await aside(browser).$("button*=Close").click();
      await openArtifact(browser, "a:ship", "Artifact ship");
      await browser.waitUntil(async () => (await sections()).Neighbours === true && (await sections()).Path === false,
        { timeout: 15000, timeoutMsg: `ship's Detail keeps the folds: ${JSON.stringify(await sections())}` });
      // An eval's Summary.
      await openTab(browser, "e:ship/check", { row: true });
      await browser.waitUntil(async () => (await sections()).Result === true, { timeout: 15000, timeoutMsg: "ship/check's Result unfolded" });
      await toggle("Result", false);
      await toggle("Result", true);
      await toggle("Fingerprint", true);
      // A Run's Detail.
      await aside(browser).$("button*=Close").click();
      await browser.$(`${MAIN} > header button.headline`).click();
      await browser.waitUntil(async () => (await sections()).Outcome === true, { timeout: 15000, timeoutMsg: "the Run's Outcome unfolded" });
      await toggle("Outcome", false);
      // A Human review's Tools, Tool log and Result never fold, even with an eval's Result left
      // folded: they have no header button and show what they hold.
      await aside(browser).$("button*=Close").click();
      await openArtifact(browser, "a:ship", "Artifact ship");
      await openTab(browser, "e:ship/check", { row: true });
      await toggle("Result", false);
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      const review = () => browser.execute(() => Object.fromEntries([...document.querySelectorAll<HTMLElement>('aside[aria-label="Detail"] [aria-label="Human review"] .section')]
        .filter((section) => ["Tools", "Tool log", "Result"].includes(section.getAttribute("aria-label") ?? ""))
        .map((section) => [section.getAttribute("aria-label"), { folds: !!section.querySelector(":scope > button.head"), shown: section.querySelector(":scope > .body")?.checkVisibility({ opacityProperty: true, visibilityProperty: true }) === true }])));
      await browser.waitUntil(async () => {
        const shown = await review();
        return ["Tools", "Tool log", "Result"].every((section) => shown[section]?.shown === true);
      }, { timeout: 15000, timeoutMsg: "the one-column review has Tools, Tool log and Result" });
      for (const [name, section] of Object.entries(await review())) {
        assert.deepEqual(section, { folds: false, shown: true }, `the review's ${name} does not fold`);
      }
    },
    async (browser) => {
      // The next launch keeps them.
      const sections = () => detailSections(browser);
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:release", "Artifact release");
      await browser.waitUntil(async () => {
        const shown = await sections();
        return shown.Evals === true && shown.Neighbours === true && shown.Path === false;
      }, { timeout: 15000, timeoutMsg: `release's folds kept after the app quit: ${JSON.stringify(await sections())}` });
      await openTab(browser, "e:release/signoff");
      await aside(browser).$("button*=Close").click();
      await browser.$(`${MAIN} > header button.headline`).click();
      await browser.waitUntil(async () => (await sections()).Outcome === false, { timeout: 15000, timeoutMsg: "the Run's Outcome kept folded" });
    },
  ),
];
