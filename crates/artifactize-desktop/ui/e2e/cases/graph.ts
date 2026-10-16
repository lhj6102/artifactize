// The monitor's Run graph in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them, follows a submission made by
// another process live without moving any node while zoomed into release, still opens the
// highlighted eval's Detail afterwards, and moves between connected Artifacts by keyboard.
import assert from "node:assert/strict";
import { artifactize, create, createChain, createEvals, signoff } from "../fixture.ts";
import { press } from "../keys.ts";
import { breadcrumbNames } from "../breadcrumb.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type GraphAppearance } from "./graph-appearance.ts";

function chainGraph(browser: WebdriverIO.Browser) {
  const focused = () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);
  const nodeView = (id: string) => browser.execute((want) => {
    const node = document.querySelector<HTMLElement>(`.graph .svelte-flow__node[data-id="${want}"]`);
    const box = node?.getBoundingClientRect();
    const canvas = document.querySelector(".graph .canvas")?.getBoundingClientRect();
    return {
      focused: !!node && document.activeElement === node,
      visible: !!box && !!canvas && box.width > 0 && box.height > 0 &&
        box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom,
      whole: !document.querySelector('.graph [role="listbox"][aria-label="Evals"]') && !document.querySelector(".graph.split"),
    };
  }, id);
  return { focused, nodeView };
}

function evalGraph(browser: WebdriverIO.Browser) {
  const options = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[role="listbox"][aria-label="Evals"] [role="option"]')].map(
        (option) => `${option.getAttribute("aria-selected")}:${(option as HTMLElement).innerText.split("\n")[1] ?? ""}`,
      ),
    );

  const selected = async () => (await options()).findIndex((option) => option.startsWith("true"));

  const evals = () => browser.$('[role="listbox"][aria-label="Evals"]');

  const detailClosed = async () => !(await browser.$("button*=Close").isExisting());
  return { options, selected, evals, detailClosed };
}

export const scenarios: Scenario[] = [
  // UX edges: E-GRAPH-OPEN E-GRAPH-RECOLOUR E-GRAPH-ZOOM-IN E-GRAPH-EVAL-OPEN
  scenario<Record<string, never>>(
    "graph-live-verdicts",
    "lights Artifacts and edges, recolours them in place and opens a zoomed eval after a submission",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const graph = () => graphAppearance(browser);
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
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

      // Zoom into release: its evals, the sign-off highlighted and focused.
      await browser.$('.svelte-flow__node[data-id="a:release"]').click();
      const evals = await browser.$('[role="listbox"][aria-label="Evals"]');
      await evals.waitForExist({ timeout: 10000 });
      await browser.waitUntil(async () => (await browser.execute((list) => (list as HTMLElement).innerText, evals)).includes("signoff"), { timeout: 15000, timeoutMsg: "the release evals" });
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"), { timeout: 15000, timeoutMsg: "focus in the evals" });

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
      assert.equal(await evals.isFocused(), true, "a live graph update does not steal focus from the evals");
      assert.ok(recoloured);
      const after = recoloured;
      assert.notEqual(after.nodes["a:release"].border, before.nodes["a:release"].border, "the result changed the actual node border colour");
      assert.notEqual(after.nodes["a:release"].dot, before.nodes["a:release"].dot, "the result changed the actual node verdict dot colour");
      assert.notEqual(after.edges["a:release>a:ship"].stroke, before.edges["a:release>a:ship"].stroke, "the result changed the visible relation's actual stroke colour");
      for (const node of ["a:release", "a:ship", "a:docs"]) {
        assert.equal(after.nodes[node].at, before.nodes[node].at, `${node} kept its place`);
      }

      // The tree folded the completed Artifact; Enter still opens the highlighted sign-off's
      // Detail (an Artifact's Detail has no Human review).
      const review = () => browser.$('[aria-label="Human review"]').isExisting();
      await press(browser, ["Enter"], review, "the sign-off's Human review");
      // Back to the graph: Esc leaves Detail, a click on the list and Esc zooms out.
      await press(browser, ["Escape"], async () => !(await review()), "Detail left");
      await evals.click();
      await press(browser, ["Escape"], async () => !(await evals.isExisting()), "zoomed out");
    },
  ),

  // UX edges: E-GRAPH-OPEN E-GRAPH-CLOSE E-GRAPH-MOVE E-MON-TREE-MOVE
  scenario<Record<string, never>>(
    "graph-tree-focus",
    "keeps the selected Artifact and keyboard focus through repeated tree/graph switches without a mouse",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const tree = () => browser.$('[role="tree"][aria-label="Artifacts and evals"]');
      const selected = () => browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("data-path");
      const on = (id: string) => () => browser.execute((want) =>
        document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === want, id);
      await browser.waitUntil(() => browser.$('[role="listbox"][aria-label="Runs"]').isFocused(), { timeout: 15000, timeoutMsg: "the initial Runs pane focused" });
      await press(browser, ["Enter"], () => tree().isFocused(), "the Run tree focused");

      // Navigate the actual visible rows to ship, not a DOM focus or selection mutation.
      const paths = await browser.execute(() => [...document.querySelectorAll('[role="treeitem"]')]
        .map((row) => row.getAttribute("data-path")));
      const from = paths.indexOf(await selected());
      const to = paths.indexOf("a:ship");
      assert.ok(from >= 0 && to >= 0, "the selected row and ship are visible in this Run");
      const step = Math.sign(to - from);
      for (let at = from; at !== to;) {
        at += step;
        const path = paths[at];
        await press(browser, [step > 0 ? "ArrowDown" : "ArrowUp"], async () => (await selected()) === path, `tree selection on ${path}`);
      }

      await press(browser, ["v"], on("a:ship"), "the selected ship Artifact focused in the graph");
      await breadcrumbNames(browser, "ship");
      await press(browser, ["v"], async () => await tree().isFocused() && (await selected()) === "a:ship", "ship still selected and the tree focused");
      await press(browser, ["v"], on("a:ship"), "ship focused after another switch to the graph");
      await press(browser, ["ArrowLeft"], on("a:release"), "release focused upstream without a mouse");
      await breadcrumbNames(browser, "release");
      await press(browser, ["v"], async () => await tree().isFocused() && (await selected()) === "a:release", "release selected and the tree focused again");
      const next = await browser.execute(() => document.querySelector('[role="treeitem"][aria-selected="true"]')?.nextElementSibling?.getAttribute("data-path"));
      assert.ok(next, "release has a following visible row");
      await press(browser, ["ArrowDown"], async () => await tree().isFocused() && (await selected()) === next, "tree navigation continues from release");
    },
  ),

  // UX edges: E-GRAPH-MOVE E-GRAPH-CLOSE
  scenario<Record<string, never>>(
    "graph-connected-navigation",
    "moves along edges with the arrow keys and switches back to the tree with v",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
      await browser.$('.svelte-flow__node[data-id="a:release"]').click();
      await press(browser, ["Escape"], async () => !(await browser.$('[role="listbox"][aria-label="Evals"]').isExisting()), "this case's whole graph");
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === "a:release"), { timeout: 15000, timeoutMsg: "focus back on release" });
      const on = (id: string) => () =>
        browser.execute((want) => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === want, id);
      await press(browser, ["ArrowRight"], on("a:ship"), "focus on ship, downstream");
      await press(browser, ["ArrowLeft"], on("a:release"), "focus on release, upstream");
      await press(browser, ["v"], () => browser.$('[role="tree"]').isExisting(), "the tree");
    },
  ),

  // UX edges: E-GRAPH-BEYOND E-GRAPH-ZOOM-OUT
  scenario<Record<string, never>>(
    "graph-chain-culling",
    "walks the whole chain beyond the drawn nodes, through a zoom in and out, focus following",
    async () => {
      const fixture = createChain(85);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { focused, nodeView } = chainGraph(browser);
      const name = (index: number) => `a:n${String(index).padStart(3, "0")}`;
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("n000"), { timeout: 30000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
      await browser.$(".graph .svelte-flow__node").waitForDisplayed({ timeout: 30000 });
      // Make the chain extend beyond the visible canvas, independently of whether the
      // renderer keeps offscreen nodes mounted.
      const zoomIn = await browser.$(".svelte-flow__controls-zoomin");
      for (let step = 0; step < 12; step += 1) await zoomIn.click();
      await browser.waitUntil(async () => !(await nodeView(name(0))).visible || !(await nodeView(name(84))).visible,
        { timeout: 15000, timeoutMsg: "the chain extends beyond the graph viewport" });
      // Culling mounts a margin outside the canvas too. A mounted node (such as n039
      // at the left edge) is not necessarily a pointer target: choose a wholly visible,
      // hit-testable node through read-only geometry, then use the real WebDriver click.
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
      await browser.waitUntil(async () => (await visible()) !== null, { timeout: 15000, timeoutMsg: "a zoomed graph node is actually hit-testable" });
      const first = await visible();
      assert.ok(first, "a real visible node can receive the pointer");
      console.log(`e2e graph: click visible node ${first}`);
      await browser.$(`.svelte-flow__node[data-id="${first}"]`).click();
      // Escape before the click's zoom is shown would leave the Run pane instead.
      await browser.$('[role="listbox"][aria-label="Evals"]').waitForExist({ timeout: 15000, timeoutMsg: "the clicked node zoomed in" });
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
      let at = Number(((await focused()) ?? "").slice(3));
      while (at > 0) await step("ArrowLeft", name((at -= 1)));
      for (let index = 1; index < 85; index += 1) {
        await step("ArrowRight", name(index));
        if (index === 40) {
          // Into the revealed Artifact's evals and back out: focus returns to it and stays
          // through the camera's move, and the keys go on from it.
          await press(
            browser,
            ["Enter"],
            () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"),
            "the evals",
          );
          // Escape returns to the whole graph with this Artifact in view; the UX
          // does not require a particular camera scale or a disabled zoom control.
          await step("Escape", name(40));
          await breadcrumbNames(browser, "n040");
        }
      }
      // The tree's selection is the focused node: the breadcrumb names it.
      await breadcrumbNames(browser, "n084");
      const last = await nodeView(name(84));
      assert.ok(last.whole && last.visible && last.focused, "the final Artifact is visible and focused in the whole graph");
      assert.ok(revealedBeyond, "arrow navigation brought an Artifact from beyond the viewport into view and focus");
    },
  ),

  // UX edges: E-GRAPH-ZOOM-IN E-GRAPH-EVAL-SELECT E-GRAPH-EVAL-OPEN E-GRAPH-ZOOM-OUT
  scenario<Record<string, never>>(
    "graph-eval-selection",
    "selects an eval with the arrows, opens its Detail with Enter and zooms out with Esc",
    async () => {
      const fixture = createEvals(["alpha", "beta", "gamma"]);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { options, selected, evals, detailClosed } = evalGraph(browser);
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("alpha"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
      await browser.$('.svelte-flow__node[data-id="a:app"]').waitForExist({ timeout: 15000 });
      await browser.$('.svelte-flow__node[data-id="a:app"]').click();
      await evals().waitForExist({ timeout: 10000 });
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"), { timeout: 15000, timeoutMsg: "focus in the evals" });
      const first = await selected();
      await press(browser, ["ArrowDown"], async () => (await selected()) === first + 1, "the next eval selected");
      // The Detail follows the selection without being opened.
      const chosen = (await options())[first + 1].split(":")[1].trim();
      // Search only the Monitor's fourth pane: the graph's Evals list already contains
      // every title and cannot establish that the Detail followed the selection.
      const peek = () => browser.execute(() => (document.querySelectorAll(".monitor section.panel")[3] as HTMLElement).innerText);
      await browser.waitUntil(async () => (await peek()).includes(`Check ${chosen}`), { timeout: 15000, timeoutMsg: "the Detail pane follows the selected eval" });
      assert.match(await peek(), new RegExp(`app/${chosen}`));
      await press(browser, ["Enter"], async () => !(await detailClosed()), "the Detail opened");
      await press(browser, ["Escape"], detailClosed, "the Detail left");
      await evals().click();
      await press(browser, ["Escape"], async () => !(await evals().isExisting()), "zoomed out");
    },
  ),
];
