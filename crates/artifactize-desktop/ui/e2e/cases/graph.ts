// The monitor's Run graph in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them, follows a submission made by
// another process live without moving any node while release's evals pop up, still opens the
// highlighted eval's Detail afterwards, and moves between connected Artifacts by keyboard.
import assert from "node:assert/strict";
import { artifactize, create, createChain, createEvals, requests, signoff } from "../fixture.ts";
import { press } from "../keys.ts";
import { focusedNode, graphShown, nodeAt, popup, rowAt, summaryOpen, summaryShows } from "../graph-view.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type GraphAppearance } from "./graph-appearance.ts";

function chainGraph(browser: WebdriverIO.Browser) {
  const nodeView = (id: string) => browser.execute((want) => {
    const node = document.querySelector<HTMLElement>(`.graph .svelte-flow__node[data-id="${want}"]`);
    const box = node?.getBoundingClientRect();
    const canvas = document.querySelector(".graph .canvas")?.getBoundingClientRect();
    return {
      focused: !!node && document.activeElement === node,
      visible: !!box && !!canvas && box.width > 0 && box.height > 0 &&
        box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom,
      whole: !document.querySelector('[role="listbox"][aria-label="Evals"]'),
    };
  }, id);
  return { nodeView };
}

const detail = (browser: WebdriverIO.Browser) => browser.$('aside[aria-label="Detail"]');

export const scenarios: Scenario[] = [
  // UX edges: E-GRAPH-RECOLOUR E-GRAPH-EVALS-OPEN E-GRAPH-EVALS-DETAIL E-MON-DETAIL-BACK E-GRAPH-EVALS-CLOSE
  scenario<Record<string, never>>(
    "graph-live-verdicts",
    "lights Artifacts and edges, recolours them in place and opens a popup's eval after a submission",
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

      // A click pops up release's evals beside it, the sign-off highlighted and focused.
      await browser.$(nodeAt("a:release")).click();
      await browser.waitUntil(async () => {
        const shown = await popup(browser);
        return shown?.artifact === "release" && shown.focused && shown.selected === "a:release/e:release/signoff";
      }, { timeout: 15000, timeoutMsg: "release's evals popped up, the sign-off focused" });
      assert.deepEqual((await popup(browser))?.evals, ["signoff"], "release's own evals");

      // Another process signs the release off; the change hint recolours the graph.
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
      assert.equal((await popup(browser))?.focused, true, "a live graph update does not steal focus from the popup");
      assert.ok(recoloured);
      const after = recoloured;
      assert.notEqual(after.nodes["a:release"].border, before.nodes["a:release"].border, "the result changed the actual node border colour");
      assert.notEqual(after.nodes["a:release"].dot, before.nodes["a:release"].dot, "the result changed the actual node verdict dot colour");
      assert.notEqual(after.edges["a:release>a:ship"].stroke, before.edges["a:release>a:ship"].stroke, "the result changed the visible relation's actual stroke colour");
      for (const node of ["a:release", "a:ship", "a:docs"]) {
        assert.equal(after.nodes[node].at, before.nodes[node].at, `${node} kept its place`);
      }

      // Enter still opens the highlighted sign-off's Detail: its completed Human review.
      const review = () => browser.$('[aria-label="Human review"]').isExisting();
      await press(browser, ["Enter"], review, "the sign-off's Human review");
      await browser.waitUntil(async () => /completed result/i.test(await detail(browser).getText()), { timeout: 15000, timeoutMsg: "the completed review" });
      // Back to the graph as it was: Esc leaves the Detail, the popup still focused; Esc again
      // closes the popup with release focused.
      await press(browser, ["Escape"], async () => !(await review()) && (await popup(browser))?.focused === true, "Detail left, back in the popup");
      await press(browser, ["Escape"], async () => (await popup(browser)) === null && (await focusedNode(browser)) === "a:release", "popup closed on release");
    },
  ),

  // UX edges: E-MON-RUNS-GRAPH E-GRAPH-MOVE E-MON-GRAPH-TO-RUNS E-MON-DETAIL-ARTIFACT E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "graph-keyboard-focus",
    "takes the keyboard on the selected Artifact with its summary, moves without a mouse and opens an Artifact's Detail",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      const runs = () => browser.$('[role="listbox"][aria-label="Runs"]');
      await browser.waitUntil(() => runs().isFocused(), { timeout: 15000, timeoutMsg: "the initial Runs pane focused" });
      // The Run starts on the release sign-off: its Artifact takes the keyboard.
      assert.equal(await summaryOpen(browser), false, "no summary while Runs has the keyboard");
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:release", "release focused in the graph");
      await summaryShows(browser, nodeAt("a:release"), /release/);
      await press(browser, ["ArrowRight"], async () => (await focusedNode(browser)) === "a:ship", "ship focused downstream");
      await summaryShows(browser, nodeAt("a:ship"), /ship/);
      // `o` opens the selected Artifact's own Detail; Esc puts the keyboard back on it.
      await press(browser, ["o"], () => detail(browser).isExisting(), "ship's Detail opened");
      await browser.waitUntil(async () => /ship/.test(await detail(browser).$("h2").getText()), { timeout: 15000, timeoutMsg: "the Detail is ship's" });
      await press(browser, ["Escape"], async () => !(await detail(browser).isExisting()) && (await focusedNode(browser)) === "a:ship", "Detail closed, ship focused");
      await press(browser, ["ArrowLeft"], async () => (await focusedNode(browser)) === "a:release", "release focused upstream");
      await press(browser, ["Escape"], () => runs().isFocused(), "back to Runs");
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:release", "release, still selected, focused again");
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
      await browser.$('button[title="Open the Run\'s Detail"]').click();
      await browser.waitUntil(async () => (await detail(browser).isExisting()) && (await detail(browser).getText()).includes(run),
        { timeout: 15000, timeoutMsg: "the Run's own Detail" });
      await detail(browser).$("button*=Close").click();
      await browser.waitUntil(async () => !(await detail(browser).isExisting()), { timeout: 15000, timeoutMsg: "the Detail closed" });
      await graphShown(browser, "a:release");
    },
  ),

  // UX edges: E-GRAPH-BEYOND E-GRAPH-EVALS-CLOSE
  scenario<Record<string, never>>(
    "graph-chain-culling",
    "walks the whole chain beyond the drawn nodes, through a popup opened and closed, focus following",
    async () => {
      const fixture = createChain(85);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { nodeView } = chainGraph(browser);
      const name = (index: number) => `a:n${String(index).padStart(3, "0")}`;
      // Culling also mounts endpoints outside the canvas. Choose a wholly visible,
      // hit-testable node from the current drawing, not a retained WebDriver element.
      const visible = () => browser.execute(() => {
        const canvas = document.querySelector(".graph .canvas")?.getBoundingClientRect();
        if (!canvas) return null;
        return [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => {
          const box = node.getBoundingClientRect();
          const x = box.left + box.width / 2;
          const y = box.top + box.height / 2;
          return { node, box, x, y, distance: Math.abs(x - (canvas.left + canvas.width / 2)) };
        }).filter(({ node, box, x, y }) => box.width > 0 && box.height > 0 &&
          box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom &&
          document.elementFromPoint(x, y)?.closest(".svelte-flow__node") === node)
          .sort((a, b) => a.distance - b.distance)[0]?.node.dataset.id ?? null;
      });
      // The initial fit can cull the first mounted node. Waiting on that element holds a
      // detached reference forever in the embedded driver; observe the live drawing instead.
      await browser.waitUntil(async () => (await visible()) !== null,
        { timeout: 30000, timeoutMsg: "a graph node is visible and hit-testable after the initial fit" });
      // Make the chain extend beyond the visible canvas, independently of whether the
      // renderer keeps offscreen nodes mounted.
      const zoomIn = await browser.$(".svelte-flow__controls-zoomin");
      for (let step = 0; step < 12; step += 1) await zoomIn.click();
      await browser.waitUntil(async () => !(await nodeView(name(0))).visible || !(await nodeView(name(84))).visible,
        { timeout: 15000, timeoutMsg: "the chain extends beyond the graph viewport" });
      await browser.waitUntil(async () => (await visible()) !== null, { timeout: 15000, timeoutMsg: "a zoomed graph node is actually hit-testable" });
      const first = await visible();
      assert.ok(first, "a real visible node can receive the pointer");
      console.log(`e2e graph: click visible node ${first}`);
      await browser.$(nodeAt(first)).click();
      // Escape before the click's popup is shown would leave the Run pane instead.
      await browser.$('[role="listbox"][aria-label="Evals"]').waitForExist({ timeout: 15000, timeoutMsg: "the clicked node's evals popped up" });
      await press(browser, ["Escape"], async () => {
        const shown = await nodeView(first);
        return shown.whole && shown.focused && shown.visible;
      }, "the whole graph with its selected Artifact visible and focused");
      let revealedBeyond = false;
      /** Each arrow must bring its target into the viewport as well as moving focus.
       * A missing/offscreen target before that key is the actual beyond-view boundary. */
      const step = async (key: string, want: string) => {
        const before = await nodeView(want);
        await press(browser, [key], async () => {
          const shown = await nodeView(want);
          return shown.whole && shown.focused && shown.visible;
        }, `${want} selected, visible and focused in the whole graph`);
        if (key.startsWith("Arrow") && !before.visible) revealedBeyond = true;
      };
      // Back to the start of the chain, then along all of it.
      let at = Number(((await focusedNode(browser)) ?? "").slice(3));
      while (at > 0) await step("ArrowLeft", name((at -= 1)));
      for (let index = 1; index < 85; index += 1) {
        await step("ArrowRight", name(index));
        if (index === 40) {
          // The revealed Artifact's evals pop up and close: focus returns to it and stays,
          // and the keys go on from it.
          await press(
            browser,
            ["Enter"],
            async () => (await popup(browser))?.focused === true,
            "the evals",
          );
          await step("Escape", name(40));
          await summaryShows(browser, nodeAt(name(40)), /n040/);
        }
      }
      // The selection is the focused node: the summary beside it names it.
      await summaryShows(browser, nodeAt(name(84)), /n084/);
      const last = await nodeView(name(84));
      assert.ok(last.whole && last.visible && last.focused, "the final Artifact is visible and focused in the whole graph");
      assert.ok(revealedBeyond, "arrow navigation brought an Artifact from beyond the viewport into view and focus");
    },
  ),

  // UX edges: E-GRAPH-EVALS-OPEN E-GRAPH-EVALS-SELECT E-GRAPH-EVALS-DETAIL E-MON-DETAIL-TECHNICAL
  // E-MON-DETAIL-BACK E-GRAPH-EVALS-CLOSE
  scenario<Record<string, never>>(
    "graph-eval-selection",
    "pops up an Artifact's evals with Enter, selects one with the arrows, opens its Detail with its technical record and closes both",
    async () => {
      const fixture = createEvals(["alpha", "beta", "gamma"]);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:app");
      await browser.waitUntil(() => browser.$('[role="listbox"][aria-label="Runs"]').isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:app", "app focused in the graph");
      await press(browser, ["Enter"], async () => (await popup(browser))?.focused === true, "app's evals popped up");
      const shown = (await popup(browser))!;
      assert.deepEqual(shown.evals, ["alpha", "beta", "gamma"], "app's own evals, with their verdicts");
      const index = shown.evals.findIndex((name) => shown.selected === `a:app/e:app/${name}`);
      assert.ok(index >= 0 && index + 1 < shown.evals.length, "an eval with another after it is selected");
      const chosen = shown.evals[index + 1];
      const path = `a:app/e:app/${chosen}`;
      await press(browser, ["ArrowDown"], async () => (await popup(browser))?.selected === path, "the next eval selected");
      // Its summary follows the selection, beside its row.
      await summaryShows(browser, rowAt(path), new RegExp(`Check ${chosen}`));
      await press(browser, ["Enter"], () => detail(browser).isExisting(), "the Detail opened");
      // The selected eval's own instruction, result and request, then its technical record.
      const request = requests(fixture).find((saved) => saved.evalId === `app/${chosen}`);
      assert.ok(request?.result, `the real CLI saved app/${chosen}'s result`);
      const field = (name: string) => browser.execute((key) => [...document.querySelectorAll('[role="document"] dt')]
        .find((entry) => entry.textContent === key)?.nextElementSibling?.textContent ?? "", name);
      await browser.waitUntil(async () => (await field("Request")) === request.id, { timeout: 15000, timeoutMsg: `the Detail is app/${chosen}'s` });
      assert.ok((await browser.$('[role="document"]').getText()).includes(`Check ${chosen}.`), "its instruction");
      assert.deepEqual(JSON.parse(await field("Raw result")), request.result, "its exact saved result");
      const technical = () => browser.$("button*=Technical").getAttribute("aria-expanded");
      assert.equal(await field("Fingerprint"), "", "the technical record starts folded");
      await press(browser, ["t"], async () => (await technical()) === "true" && (await field("Fingerprint")) !== "", "the technical record shown");
      assert.equal(await field("Request"), request.id, "the request stays shown beside the technical record");
      await press(browser, ["t"], async () => (await technical()) === "false" && (await field("Fingerprint")) === "", "the technical record hidden");
      // Back to the graph as it was: the popup, the chosen eval still selected.
      await press(browser, ["Escape"], async () => !(await detail(browser).isExisting()) && (await popup(browser))?.focused === true, "the Detail left");
      assert.equal((await popup(browser))?.selected, path, "the chosen eval is still selected");
      await press(browser, ["Escape"], async () => (await popup(browser)) === null && (await focusedNode(browser)) === "a:app", "the popup closed on app");
    },
  ),
];
