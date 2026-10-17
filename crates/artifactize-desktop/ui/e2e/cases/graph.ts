// The monitor's Run graph in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them and follows a submission made by
// another process live without moving any node while release's Detail stays open; a click on an
// Artifact opens its Detail with a tab per eval, and the camera brings a large Run's Artifacts
// into view.
import assert from "node:assert/strict";
import { artifactize, create, createChain, createEvals, requests, signoff } from "../fixture.ts";
import { press } from "../keys.ts";
import { aside, detail, fact, focusedNode, graphShown, openArtifact, openTab, summary, tabAt } from "../graph-view.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type GraphAppearance } from "./graph-appearance.ts";

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
      assert.ok(opened.tabs[0].icon, "release's chip draws the Artifact icon before its name");
      assert.equal(opened.tabs[1].tone, "tone-human", "the sign-off's tab shows it waiting for a Human");
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
      await browser.waitUntil(async () => (await detail(browser))?.tabs[1]?.tone === "tone-green",
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

  // UX edges: E-MON-RUNS-GRAPH E-MON-DETAIL-ARTIFACT E-MON-DETAIL-SWITCH E-MON-DETAIL-BACK E-MON-GRAPH-TO-RUNS
  scenario<Record<string, never>>(
    "graph-click-detail",
    "opens an Artifact's Detail on a click or Enter, switches it on another click, closes it on the background, then steps back to Runs",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      const runs = () => browser.$('[role="listbox"][aria-label="Runs"]');
      const pane = () => browser.$('section.panel[aria-label^="Run run-"]');
      await browser.waitUntil(() => runs().isFocused(), { timeout: 15000, timeoutMsg: "the initial Runs pane focused" });
      // Enter steps from Runs into the graph's pane.
      await press(browser, ["Enter"], () => pane().isFocused(), "the graph's pane focused");

      // A click opens an Artifact's Detail and leaves the keyboard on that Artifact: Esc closes
      // the Detail, Enter on the focused Artifact opens it again, as the click did.
      await openArtifact(browser, "a:ship", "Artifact ship");
      await browser.waitUntil(async () => (await focusedNode(browser)) === "a:ship", { timeout: 15000, timeoutMsg: "the keyboard on ship" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === "Artifact ship", "Enter on ship opened its Detail again");

      // Another click switches the Detail to that Artifact, and a click on the graph's
      // background closes it. Esc then steps back to Runs.
      await openArtifact(browser, "a:release", "Artifact release");
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
      await press(browser, ["Escape"], () => runs().isFocused(), "back to Runs");
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
      const run = await browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");
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

  // UX edges: E-GRAPH-BEYOND
  scenario<Record<string, never>>(
    "graph-chain-culling",
    "draws the Artifacts the camera brings into view when zooming over a chain too large to draw at once",
    async () => {
      const fixture = createChain(85);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
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
      const drawn = (id: string) => browser.execute((want) => !!document.querySelector(`.svelte-flow__node[data-id="${want}"]`), id);
      await browser.waitUntil(async () => (await visible()).length > 0,
        { timeout: 30000, timeoutMsg: "a graph node is visible after the initial fit" });
      // Zoom in until the chain extends beyond the canvas and its last Artifact is not drawn.
      const zoomIn = await browser.$(".svelte-flow__controls-zoomin");
      for (let step = 0; step < 12; step += 1) await zoomIn.click();
      await browser.waitUntil(async () => !(await drawn(name(84))) && !(await visible()).includes(name(84)),
        { timeout: 15000, timeoutMsg: "the chain's end lies beyond the canvas and is not drawn" });
      // Fit View moves the camera back over the whole chain: the Artifacts it brings into view
      // are drawn, the chain's end among them.
      await browser.$(".svelte-flow__controls-fitview").click();
      await browser.waitUntil(async () => (await visible()).includes(name(84)), { timeout: 15000, timeoutMsg: "the camera drew the chain's last Artifact in view" });
      // The Artifact brought into view takes the pointer like any other.
      await graphShown(browser, name(84));
      await openArtifact(browser, name(84), "Artifact n084");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL E-MON-DETAIL-LOG E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "graph-eval-tabs",
    "lists an Artifact's evals on its Artifact tab, shows one's Summary from the list with its Debug facts and its Runtime log, and another from the tabs",
    async () => {
      const fixture = createEvals(["alpha", "beta", "gamma"]);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:app");
      const opened = await openArtifact(browser, "a:app", "Artifact app");
      assert.deepEqual(opened.tabs.map((tab) => tab.name), ["app", "alpha", "beta", "gamma"], "app's own chip, then one per eval");
      assert.ok(opened.tabs.slice(1).every((tab) => tab.tone === "tone-green" && tab.glyph !== ""), "each eval's tab shows its GREEN glyph in GREEN");
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
      // Another eval's tab shows that eval; Esc closes the Detail.
      await openTab(browser, "e:app/gamma");
      await browser.waitUntil(async () => (await summary(browser)).instruction.includes("Check gamma."), { timeout: 15000, timeoutMsg: "the Detail is app/gamma's" });
      assert.equal(await browser.$(tabAt("e:app/gamma")).getAttribute("aria-selected"), "true");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-NEIGHBOUR
  scenario<Record<string, never>>(
    "graph-detail-neighbours",
    "draws an Artifact's Detail between the Artifacts its evals use and those using it, and opens a neighbour's Detail on a click",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      // Each node drawn in the Detail's neighbours, left to right: the Artifacts its evals use,
      // the Artifact itself, then those whose evals use it.
      const neighbours = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('aside[aria-label="Detail"] [role="group"][aria-label="Neighbours"] .svelte-flow__node')]
        .map((node) => ({ id: node.getAttribute("data-id") ?? "", x: node.getBoundingClientRect().left }))
        .sort((a, b) => a.x - b.x).map((node) => node.id));
      await openArtifact(browser, "a:release", "Artifact release");
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "release drawn before ship, which uses it" });
      const neighbour = (id: string) => aside(browser).$(`[aria-label="Neighbours"] .svelte-flow__node[data-id="${id}"]`);
      await neighbour("a:ship").click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact ship", { timeout: 15000, timeoutMsg: "ship's Detail opened from its neighbour" });
      assert.equal((await detail(browser))?.tabs[0]?.selected, true, "on its Artifact tab");
      await browser.waitUntil(async () => (await neighbours()).join() === "a:release,a:ship", { timeout: 15000, timeoutMsg: "ship drawn after release, which it uses" });
      await neighbour("a:release").click();
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact release", { timeout: 15000, timeoutMsg: "release's Detail again" });
    },
  ),
];
