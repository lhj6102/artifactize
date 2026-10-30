// A Run's graph in the Main area, in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them and follows a submission made by
// another process live without moving any node while release's Detail stays open; a click on an
// Artifact opens its Detail with a tab per eval, floating over the graph, whose camera ignores it,
// and the camera brings a large Run's Artifacts into view.
import assert from "node:assert/strict";
import { artifactize, create, createChain, createEvals, createParts, requests, savedRuns, signOff, signoff } from "../fixture.ts";
import { afterRender, fill, press } from "../keys.ts";
import { visibleCenter } from "../pointer.ts";
import { dispatchInput, tab } from "../input.ts";
import { aside, camera, detail, fact, FIND, findField, focusedNode, graphShown, offCentre, openArtifact, openArtifactTab, openTab, summary, tabAt } from "../graph-view.ts";
import { primaryModifier } from "../platform.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type GraphAppearance } from "./graph-appearance.ts";
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

/** How wide an Artifact's node draws at the zoom its name starts to read at: 168 at zoom 1, at
 * three quarters. */
const READABLE = 126;

/** The copies of {@link createParts}'s graph: the wholes suite and docs, app inside suite, kit
 * inside app, site inside both, and lib outside every whole. */
const PARTS = ["a:suite", "a:suite/a:app", "a:suite/a:app/a:kit", "a:suite/a:site", "a:docs", "a:docs/a:site", "a:lib"];

/** The copies of the graph under `root`, outside the Detail, once all of {@link PARTS} are
 * drawn: each one's box, whether it is drawn as a whole, its title text, its Artifact, its
 * spoken name, whether it shows as opened, and its light. */
async function partsDrawn(browser: WebdriverIO.Browser, root: string) {
  const read = () => browser.execute((under) => Object.fromEntries([...document.querySelectorAll<HTMLElement>(`${under} .svelte-flow__node`)]
    .filter((node) => !node.closest('aside[aria-label="Detail"]'))
    .map((node) => [node.dataset.id ?? "", {
      rect: node.getBoundingClientRect().toJSON() as DOMRect,
      whole: node.classList.contains("svelte-flow__node-whole"),
      text: node.textContent?.trim() ?? "",
      artifact: node.dataset.artifact ?? "",
      label: node.getAttribute("aria-label") ?? "",
      open: !!node.querySelector(".open"),
      light: node.dataset.light ?? "",
    }])), root);
  let shown: Awaited<ReturnType<typeof read>> = {};
  await browser.waitUntil(async () => {
    shown = await read();
    return PARTS.every((key) => key in shown);
  }, { timeout: 15000, timeoutMsg: "every whole and part drawn" });
  return shown;
}

/** A rectangle wholly inside another. */
const within = (inner: DOMRect, outer: DOMRect) =>
  inner.left >= outer.left && inner.top >= outer.top && inner.right <= outer.right && inner.bottom <= outer.bottom;

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

export const scenarios: Scenario[] = [
  // UX edges: E-GRAPH-RECOLOUR E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-BACK
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
      }, { timeout: 15000, timeoutMsg: "release, ship, docs and the relation are painted in their verdict's distinct Legend colours" });
      assert.ok(drawn);
      const before = drawn;

      // A click opens release's Detail on its Artifact tab: its one eval, waiting for a Human.
      const opened = await openArtifact(browser, "a:release", "Artifact release");
      assert.deepEqual(opened.tabs.map((tab) => tab.name), ["release", "signoff"], "release's own chip, then its one eval's");
      assert.equal(opened.path, "release / signoff", "the chips read as a path");
      assert.equal(opened.tabs[0].label, "Artifact release", "release's chip is read as the Artifact release");
      await browser.waitUntil(async () => (await detail(browser))?.tabs[0]?.icon === true,
        { timeout: 15000, timeoutMsg: "release's chip draws the Artifact icon before its name" });
      assert.equal(opened.tabs[1].glyph, "?", "the sign-off's tab shows it waiting for a Human");
      assert.deepEqual(opened.evals.map((row) => row.name), ["signoff"], "the Artifact tab lists release's evals");
      assert.ok(opened.docked, "the Detail floats over the graph's right side with a gap");

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
      }, { timeout: 15000, timeoutMsg: "the submitted release's node border/dot and relation stroke are painted GREEN" });
      assert.ok(recoloured);
      const after = recoloured;
      assert.notEqual(after.nodes["a:release"].border, before.nodes["a:release"].border, "the result changed the actual node border colour");
      assert.notEqual(after.nodes["a:release"].dot, before.nodes["a:release"].dot, "the result changed the actual node verdict dot colour");
      assert.notEqual(after.edges["a:release>a:ship"].stroke, before.edges["a:release>a:ship"].stroke, "the result changed the visible relation's actual stroke colour");
      for (const node of ["a:release", "a:ship", "a:docs"]) {
        assert.equal(after.nodes[node].at, before.nodes[node].at, `${node} kept its place`);
      }
      await browser.waitUntil(async () => (await detail(browser))?.tabs[1]?.glyph === "✓",
        { timeout: 15000, timeoutMsg: "the open Detail's sign-off tab turned GREEN" });
      assert.equal((await detail(browser))?.title, "Artifact release", "a live update keeps the open Detail on release");

      // The sign-off's tab holds its completed Human review; Esc closes the Detail.
      await openTab(browser, "e:release/signoff");
      const review = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"][tabindex="0"]');
      await browser.waitUntil(async () => (await review().isExisting()) && /completed result/i.test(await aside(browser).getText()),
        { timeout: 15000, timeoutMsg: "the completed review on the sign-off's tab" });
      await review().click();
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      await graphShown(browser, "a:release");
    },
  ),

  // UX journey: J-DETAIL; UX edges: E-MON-RUNS-GRAPH E-MON-DETAIL-ARTIFACT E-MON-DETAIL-SWITCH E-MON-DETAIL-BACK E-MON-GRAPH-TO-RUNS
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

      // The Detail floats over the graph's right side with a gap, docked where another Artifact
      // was drawn without it, and the graph stays visible on its left.
      assert.ok((await detail(browser))?.docked, "the Detail docks over the graph's right side with a gap");
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

  // UX journey: J-GRAPH-PARTS; UX edges: E-GRAPH-PARTS E-GRAPH-WHOLE E-GRAPH-PART E-MON-DETAIL-BACK
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
      assert.equal(drawn["a:suite"].label, "suite: GREEN");
      assert.deepEqual([drawn["a:suite/a:site"].artifact, drawn["a:docs/a:site"].artifact], ["a:site", "a:site"], "both copies are site");
      // Each copy of site carries site's edges: from lib outside every box into it, and out of
      // it to app, within suite and across from docs into suite.
      await browser.waitUntil(async () => graphHasColours(await graphAppearance(browser), { "a:lib": "green", "a:suite/a:site": "green", "a:docs/a:site": "green" }, {
        "a:lib>a:suite/a:site": "green",
        "a:lib>a:docs/a:site": "green",
        "a:suite/a:site>a:suite/a:app": "green",
        "a:docs/a:site>a:suite/a:app": "green",
      }), { timeout: 15000, timeoutMsg: "every copy of site's edges drawn and lit like any other" });

      // A click on suite's box outside its parts opens suite; a click on a part opens the part,
      // suite's box staying as it was.
      await clickWhole(browser, ".graph", "a:suite");
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact suite", { timeout: 15000, timeoutMsg: "the click on suite's box opened suite's Detail" });
      const suite = (await partsDrawn(browser, ".graph"))["a:suite"].rect;
      await openArtifact(browser, "a:suite/a:app/a:kit", "Artifact kit");
      assert.deepEqual((await partsDrawn(browser, ".graph"))["a:suite"].rect, suite, "suite's box stays");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes kit's Detail");
    },
  ),

  // UX journeys: J-GRAPH-PARTS J-GRAPH-FIND J-DETAIL; UX edges: E-GRAPH-PARTS E-GRAPH-TAB E-GRAPH-WHOLE E-GRAPH-PART
  // E-CURRENT-EVALS E-GRAPH-SHARED E-CURRENT-CLOSE E-GRAPH-FIND-OPEN E-GRAPH-FIND-TYPE E-GRAPH-FIND-GO
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
        const opens: Record<string, string | undefined> = { "a:suite/a:app": "app", "a:suite/a:app/a:kit": "kit" };
        const name = opens[focused];
        if (name === undefined) continue;
        await press(browser, ["Enter"], async () => (await detail(browser))?.title === `Artifact ${name}`, `Enter on ${focused} opens ${name}'s Detail`);
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
      await browser.waitUntil(async () => {
        const shown = await partsDrawn(browser, current);
        return shown["a:docs/a:site"].open && shown["a:suite/a:site"].open && !shown["a:docs"].open && !shown["a:suite"].open;
      }, { timeout: 15000, timeoutMsg: "both copies of site, and no whole, highlighted" });
      const note = await aside(browser).$("p=Drawn 2 times on the graph, inside suite and docs.").isExisting()
        || await aside(browser).$("p=Drawn 2 times on the graph, inside docs and suite.").isExisting();
      assert.ok(note, "site's Detail says it is drawn in both wholes, naming them");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closes the Current's Detail");
      assert.ok(!(await partsDrawn(browser, current))["a:suite/a:site"].open, "no copy stays highlighted");
    },
  ),

  // UX journey: J-DETAIL; UX edges: E-MON-RUNS-GRAPH E-GRAPH-TAB E-MON-DETAIL-ARTIFACT E-MON-DETAIL-BACK
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
        assert.equal((await detail(browser))?.tabs[0]?.name, name);
        visited.add(id);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the keyboard-opened Detail closes");
      }
      assert.deepEqual([...visited].sort(), [...expected.keys()].sort(), "every Artifact is reachable with Tab and Enter alone");
    },
  ),

  // UX journey: J-DETAIL; UX edges: E-MON-RUNS-GRAPH E-GRAPH-TAB E-CURRENT-EVALS E-CURRENT-CLOSE
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

  // UX edges: E-MON-DETAIL-RUN E-MON-DETAIL-BACK
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
      await browser.waitUntil(async () => (await detail(browser))?.title === `Run ${run}`,
        { timeout: 15000, timeoutMsg: "the selected Run's own Detail heading" });
      const shown = (await detail(browser))!;
      assert.deepEqual(shown.tabs, [], "the Run's Detail has no Artifact or eval tabs");
      assert.ok(shown.docked, "the Run's Detail floats over the graph's right side");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      await graphShown(browser, "a:release");
    },
  ),

  // UX journey: J-GRAPH-CAMERA; UX edges: E-GRAPH-FIRST E-GRAPH-CAMERA E-GRAPH-CAMERA-KEPT
  scenario<{ request: string }>(
    "graph-camera-stays",
    "shows the whole graph first, then leaves its camera to the person: the Detail and a new result never move it, Zoom Out and Fit View do",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: { request: signoff(fixture).id } };
    },
    async (browser, { fixture, data: { request } }) => {
      await browser.setWindowSize(1440, 872);
      await afterRender(browser);
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
      const title = await mainTitle(browser);

      // Opening a Detail, switching it to a neighbour and closing it leave the camera.
      await openArtifact(browser, "a:release", "Artifact release");
      assert.equal((await settled()).transform, first.transform, "opening the Detail moved nothing");
      await aside(browser).$('.svelte-flow__node[data-id="a:ship"]').click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "the Detail switched to ship" });
      assert.equal((await settled()).transform, first.transform, "switching the Detail moved nothing");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      assert.equal((await settled()).transform, first.transform, "closing the Detail moved nothing");

      // The person zooms out (a small graph first fits at the largest zoom); a new result then
      // relights the graph in place, under their camera, not a fit of its own.
      await browser.$(".graph .svelte-flow__controls-zoomout").click();
      const zoomed = await settled();
      assert.notEqual(zoomed.transform, first.transform, "Zoom Out moved the camera");
      signOff(fixture, request);
      await browser.waitUntil(() => browser.$('.graph .svelte-flow__node[data-id="a:release"]').getAttribute("data-light").then((light) => light === "green"),
        { timeout: 30000, timeoutMsg: "release relit GREEN" });
      assert.equal(await mainTitle(browser), title, "the same Run's graph stays shown");
      assert.equal((await settled()).transform, zoomed.transform, "a new result kept the person's camera");
      // The Run ends with nothing left needing attention: the lines above the graph go and the
      // canvas grows, and the camera still stays. The canvas keeps that size from here on.
      await browser.waitUntil(() => browser.execute(() => document.querySelector(".run .attention") === null),
        { timeout: 30000, timeoutMsg: "the Run ended with nothing needing attention" });
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

      // Fit View, the person's own, shows the whole graph in its whole canvas: the Detail floats
      // over it and takes no part, so the camera is the graph's first one again.
      await browser.$(".graph .svelte-flow__controls-fitview").click();
      const fitted = await settled();
      assert.notEqual(fitted.transform, zoomed.transform, "Fit View moved the person's camera");
      assert.ok(fitted.whole, `Fit View shows every Artifact inside the canvas: ${JSON.stringify(fitted)}`);
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      assert.equal((await settled()).transform, fitted.transform, "closing the Detail keeps what Fit View showed");
      // The person zooms out again, so the next Fit View has a camera to move back.
      await browser.$(".graph .svelte-flow__controls-zoomout").click();
      assert.notEqual((await settled()).transform, fitted.transform, "Zoom Out moved the fitted camera");
      await browser.$(".graph .svelte-flow__controls-fitview").click();
      assert.equal((await settled()).transform, fitted.transform, "Fit View without the Detail fits as with it open");

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

  // UX edges: E-GRAPH-BEYOND
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
      await browser.waitUntil(async () => (await visible()).length > 0,
        { timeout: 30000, timeoutMsg: "a graph node is visible after the initial fit" });
      // Zoom in until the chain extends beyond the canvas and its last Artifact is out of view.
      const zoomIn = await browser.$(".svelte-flow__controls-zoomin");
      for (let step = 0; step < 12; step += 1) await zoomIn.click();
      await browser.waitUntil(async () => !(await visible()).includes(name(124)),
        { timeout: 15000, timeoutMsg: "the chain's end lies beyond the canvas" });
      // Fit View moves the camera back over the whole chain: the Artifacts it brings into view
      // are drawn, the chain's end among them.
      await browser.$(".svelte-flow__controls-fitview").click();
      await browser.waitUntil(async () => (await visible()).includes(name(124)), { timeout: 15000, timeoutMsg: "the camera drew the chain's last Artifact in view" });
      // The Artifact brought into view takes the pointer like any other.
      await graphShown(browser, name(124));
      await openArtifact(browser, name(124), "Artifact n124");
    },
  ),

  // UX journey: J-DETAIL; UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL E-MON-DETAIL-LOG E-DETAIL-ARTIFACT-TAB E-MON-DETAIL-SWITCH E-MON-DETAIL-BACK
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
      assert.deepEqual(opened.tabs.map((tab) => tab.name), ["app", "alpha", "beta", "gamma"], "app's own chip, then one per eval");
      assert.ok(opened.tabs.slice(1).every((tab) => tab.glyph === "✓"), "each eval's tab shows its GREEN glyph");
      assert.deepEqual(opened.evals.map((row) => row.name), ["alpha", "beta", "gamma"], "the Artifact tab lists app's own evals");
      assert.ok(opened.evals.every((row) => /GREEN/.test(row.status)), "with their verdicts");
      // A click on an eval of the list shows its tab: its own status, instruction and result.
      await openTab(browser, "e:app/beta", { row: true });
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
      const log = () => browser.execute(() => document.querySelector('aside[aria-label="Detail"] .evidence')?.textContent ?? "");
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
      assert.equal(back.tabs[2]?.selected, false, "app/beta's tab is no longer the shown one");
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
      assert.deepEqual(other.tabs.map((tab) => tab.name), ["lib", "ready"], "lib's own chip and eval, none of app's");
      assert.equal(await log(), "", "app/gamma's log is gone");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TECHNICAL E-MON-DETAIL-NEIGHBOUR E-MON-DETAIL-TAB
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
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "release drawn before ship, which uses it" });
      const drawing = '[aria-label="Detail"] [aria-label="Neighbours"]';
      let edges: (string | null)[] = [];
      await browser.waitUntil(async () => {
        edges = await browser.execute((selector) => [...document.querySelectorAll(`${selector} .svelte-flow__edge`)].map((edge) => edge.getAttribute("data-id")), drawing);
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
      // Below the drawing: its evals with their verdicts, then its path; every other fact folds
      // under Debug, unfolded with t.
      const path = () => browser.execute(() =>
        document.querySelector('aside[aria-label="Detail"] [role="document"] .card > p.line')?.textContent?.trim() ?? "");
      await browser.waitUntil(async () => (await path()) === "release", { timeout: 15000, timeoutMsg: "release's path below its evals" });
      assert.deepEqual((await detail(browser))?.evals.map((row) => row.name), ["signoff"], "release's one eval listed");
      assert.equal((await summary(browser)).debug, false, "the Artifact's Debug starts folded");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await summary(browser)).debug === true && (await fact(browser, "Tags")) !== "", "the Artifact's Debug unfolded by t");
      const neighbour = (id: string) => aside(browser).$(`[aria-label="Neighbours"] .svelte-flow__node[data-id="${id}"]`);
      await neighbour("a:ship").click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "ship's Detail opened from its neighbour" });
      assert.equal((await detail(browser))?.tabs[0]?.selected, true, "on its Artifact tab");
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "ship drawn after release, which it uses" });
      // What ship waits for is a Debug fact, not shown above it.
      assert.equal(await browser.execute(() => [...document.querySelectorAll('aside[aria-label="Detail"] .block h3')]
        .some((heading) => heading.textContent === "Waits for")), false, "ship's waits are not shown outside Debug");
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

  // UX journeys: J-GRAPH-FIND J-PANE-SEARCH; UX edges: E-REVIEW-SEARCH-FOCUS E-REVIEW-SEARCH-LEAVE E-MON-RUNS-GRAPH
  // E-GRAPH-FIND-OPEN E-GRAPH-FIND-TYPE E-GRAPH-FIND-CLOSE E-MON-DETAIL-ARTIFACT E-GRAPH-FIND-GO E-GRAPH-FIND-CHOOSE
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
      /** The Artifact `id` sits in the middle of the graph left of the Detail, zoomed in only as
       * far as its name needs: drawn `before` wide, or {@link READABLE} wide if that was less. */
      const centred = async (id: string, before: number) => {
        const at = await offCentre(browser, id);
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && Math.abs(at.width - Math.max(before, READABLE)) <= 1;
      };
      const width = async (id: string) => (await offCentre(browser, id))?.width ?? 0;
      const inDetail = () => browser.execute(() => !!document.activeElement?.closest('aside[aria-label="Detail"]'));
      await graphShown(browser, "a:release");
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane initially has the keyboard" });

      // In the Pane, Ctrl+F focuses the Pane's search, as / does; no find field opens.
      await press(browser, [primaryModifier, "f"], () => searchFocused(browser), "Ctrl+F in the Pane focuses its search");
      assert.equal(await field(), null, "no find field over the graph");
      await press(browser, ["Escape"], () => entriesFocused(browser), "Esc on sort:recent back to the entries");
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");

      // Over the graph, Ctrl+F opens the find field, empty and focused; a text nothing is named
      // so says so, and Esc closes it, the camera and the keyboard where they were.
      const first = await camera(browser);
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open and focused");
      assert.deepEqual(await field(), { focused: true, value: "", entries: [], note: "" });
      assert.equal(await searchFocused(browser), false, "the Pane's search stays unfocused");
      await fill(browser, FIND, "nothing");
      await browser.waitUntil(async () => (await field())?.note === "No Artifact or eval matches.", { timeout: 15000, timeoutMsg: "nothing found said so" });
      assert.deepEqual(await names(), []);
      await press(browser, ["Escape"], async () => (await field()) === null, "Esc closes the find field");
      assert.ok(await mainFocused(browser), "the keyboard is back on the Main area");
      assert.equal(await camera(browser), first, "Esc moved no camera");
      assert.equal(await detail(browser), null, "Esc opened nothing");

      // With docs's Detail open, Ctrl+F finds ship; a click on it opens ship's Detail and brings
      // ship to the middle of the graph left of the Detail.
      await openArtifact(browser, "a:docs", "Artifact docs");
      await press(browser, [primaryModifier, "f"], async () => (await field())?.focused === true, "the find field open over docs's Detail");
      await fill(browser, FIND, "ship");
      await browser.waitUntil(async () => JSON.stringify(await names()) === JSON.stringify(["ship"]), { timeout: 15000, timeoutMsg: "ship found" });
      const shipBefore = await width("a:ship");
      await browser.$('[aria-label="Found"] [role="option"]').click();
      await browser.waitUntil(async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.title === "Artifact ship" && shown.tabs[0]?.selected === true;
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
      const releaseBefore = await width("a:release");
      await press(browser, ["Enter"], async () => {
        const shown = await detail(browser);
        return (await field()) === null && shown?.tabs[0]?.name === "release" && shown.tabs.find((each) => each.selected)?.name === "signoff";
      }, "Enter opens release's Detail on its signoff tab");
      const tab = (await detail(browser))!.tabs.find((each) => each.selected)!;
      assert.deepEqual([found.glyph, found.colour], [tab.glyph, tab.colour], "the found eval showed its verdict's glyph and colour");
      await browser.waitUntil(() => centred("a:release", releaseBefore), { timeout: 15000, timeoutMsg: "release in the middle of the graph left of the Detail" });
      assert.ok(await inDetail(), "the keyboard is in the Detail");
    },
  ),

  // UX journey: J-GRAPH-FIND; UX edges: E-GRAPH-FIND-OPEN E-GRAPH-FIND-TYPE E-GRAPH-FIND-GO
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
      const before = (await offCentre(browser, "a:n000"))!.width;
      assert.ok(before < READABLE, `the whole chain shows too small to read: ${before}`);
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
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && Math.abs(at.width - READABLE) <= 1;
      }, { timeout: 15000, timeoutMsg: "n000 in the middle of the graph left of the Detail, zoomed in just to read" });
      // Each listed Artifact showed what verify would do with it, reused, as its eval's chip in
      // the Detail of the Current shows it.
      const reused = (await detail(browser))!.tabs.find((each) => each.name === "check")!;
      assert.equal(reused.glyph, "↻", "the Detail's eval chip says reused");
      assert.deepEqual(shown.entries.filter((entry) => entry.glyph !== reused.glyph || entry.colour !== reused.colour), [],
        "every found Artifact showed reused, in its glyph and colour");
    },
  ),
];
