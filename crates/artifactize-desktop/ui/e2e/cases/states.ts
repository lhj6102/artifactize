// The Run graph with every kind of verdict, and the Detail of a finished eval: node and edge
// colours for RED, ERROR and blocked Artifacts, the arrows between nodes of one column, `v`
// between the tree and the graph, and the instruction, request, result and technical record
// of a completed eval.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { createStates, requests } from "../fixture.ts";
import { press } from "../keys.ts";
import { breadcrumbNames } from "../breadcrumb.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type VerdictLight } from "./graph-appearance.ts";

/** A clicked node zooms in. Escape then steps back out: it is pressed only once the zoom is
 * shown, since before it Escape leaves the Run pane, and "zoomed out" would already hold. */
async function zoomedIn(browser: WebdriverIO.Browser): Promise<void> {
  await browser.$('[role="listbox"][aria-label="Evals"]').waitForExist({ timeout: 15000, timeoutMsg: "the clicked node zoomed in" });
}

function verdictGraph(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);

  const lights = () => graphAppearance(browser);

  const focused = () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);

  const tree = () => browser.$('[role="tree"]');
  return { text, lights, focused, tree };
}

function finishedDetail(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);
  return { text };
}

export const scenarios: Scenario[] = [
  // UX edges: E-GRAPH-OPEN E-GRAPH-CLOSE
  scenario<Record<string, never>>(
    "states-tree-graph-switch",
    "switches between the tree and the graph with v",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, tree } = verdictGraph(browser);
      await browser.waitUntil(async () => (await text()).includes("crash"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await tree().click();
      await press(browser, ["v"], () => browser.$(".svelte-flow__node").isExisting(), "the graph");
      assert.ok(!(await tree().isExisting()), "the graph replaces the tree");
      // The tree's list left with the switch; the graph's own node takes the keys.
      await browser.$(".svelte-flow__node").click();
      await zoomedIn(browser);
      await press(browser, ["Escape"], async () => !(await browser.$('[role="listbox"][aria-label="Evals"]').isExisting()), "zoomed out to a focused node");
      await press(browser, ["v"], () => tree().isExisting(), "the tree again");
      await tree().click();
      await press(browser, ["v"], () => browser.$(".svelte-flow__node").isExisting(), "the graph once more");
    },
  ),

  // UX edges: E-GRAPH-OPEN
  scenario<Record<string, never>>(
    "states-graph-lights",
    "colours RED, ERROR, blocked, waiting and GREEN Artifacts and the edges each passes on to its dependent",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, lights } = verdictGraph(browser);
      await browser.waitUntil(async () => (await text()).includes("crash"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
      await browser.$(".svelte-flow__node").waitForExist({ timeout: 15000 });
      const expectedNodes = {
        "a:red": "red",
        "a:crash": "error",
        "a:down": "blocked",
        "a:after": "blocked",
        "a:fine": "green",
        "a:gate": "active",
        "a:later": "waiting",
        "a:past": "blocked",
        "a:beyond": "waiting",
      } satisfies Record<string, VerdictLight>;
      // Each edge is painted as what its source passes on: RED, ERROR, work in progress, a
      // blocked or a waiting source.
      const expectedEdges = {
        "a:red>a:down": "red", "a:crash>a:after": "error", "a:gate>a:later": "active",
        "a:down>a:past": "blocked", "a:later>a:beyond": "waiting",
      } satisfies Record<string, VerdictLight>;
      await browser.waitUntil(async () => graphHasColours(await lights(), expectedNodes, expectedEdges), {
        timeout: 15000, timeoutMsg: "every node border/dot and relation stroke matches its verdict's distinct visible Legend colour",
      });
      const { nodes, edges } = await lights();
      assert.deepEqual(Object.fromEntries(Object.entries(nodes).map(([id, node]) => [id, node.light])), expectedNodes);
      assert.deepEqual(Object.fromEntries(Object.entries(edges).map(([id, edge]) => [id, edge.light])), expectedEdges);
      const legend = await browser.$('[aria-label="Legend"]').getText();
      for (const word of ["GREEN", "RED", "ERROR", "blocked", "waiting"]) assert.match(legend, new RegExp(word));
    },
  ),

  // UX edges: E-GRAPH-MOVE
  scenario<Record<string, never>>(
    "states-graph-navigation",
    "moves along edges with ← and →, and between the nodes of one column with ↑ and ↓",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, focused } = verdictGraph(browser);
      await browser.waitUntil(async () => (await text()).includes("crash"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$("button*=Graph").click();
      await browser.$(".svelte-flow__node").waitForExist({ timeout: 15000 });
      const on = (id: string) => async () => (await focused()) === id;
      await browser.$('.svelte-flow__node[data-id="a:red"]').click();
      // A click zooms in; step back out to the whole graph with the node focused.
      await zoomedIn(browser);
      await press(browser, ["Escape"], async () => !(await browser.$('[role="listbox"][aria-label="Evals"]').isExisting()), "zoomed out");
      await browser.waitUntil(on("a:red"), { timeout: 15000, timeoutMsg: "focus on red" });
      // → the dependent, ← back upstream.
      await press(browser, ["ArrowRight"], on("a:down"), "focus on down, downstream");
      await press(browser, ["ArrowLeft"], on("a:red"), "focus on red, upstream");
      // ↑ ↓ walk the nodes of a column from top to bottom and back.
      const placed = await browser.execute(() =>
        [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => {
          const box = node.getBoundingClientRect();
          return { id: node.dataset.id ?? "", x: box.left, y: box.top };
        }),
      );
      const byColumn = new Map<number, { id: string; y: number }[]>();
      for (const node of placed) byColumn.set(Math.round(node.x), [...(byColumn.get(Math.round(node.x)) ?? []), node]);
      const order = [...byColumn.values()]
        .find((nodes) => nodes.length >= 2)!
        .sort((a, b) => a.y - b.y)
        .map((node) => node.id);
      assert.ok(order.length >= 2, "a column of at least two nodes");
      await browser.$(`.svelte-flow__node[data-id="${order[0]}"]`).click();
      await zoomedIn(browser);
      await press(browser, ["Escape"], async () => !(await browser.$('[role="listbox"][aria-label="Evals"]').isExisting()), "the first node in the column focused");
      await browser.waitUntil(on(order[0]), { timeout: 15000, timeoutMsg: `focus on ${order[0]}` });
      for (const next of order.slice(1)) {
        await press(browser, ["ArrowDown"], on(next), `focus on ${next}, below`);
        await breadcrumbNames(browser, next.slice(2));
      }
      await press(browser, ["ArrowUp"], on(order[order.length - 2]), `focus on ${order[order.length - 2]}, above`);
    },
  ),

  // UX edges: E-MON-DETAIL-TECHNICAL E-MON-TREE-DETAIL
  scenario<Record<string, never>>(
    "states-finished-detail",
    "shows the instruction, the request and the result, and the technical record only on demand",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { text } = finishedDetail(browser);
      await browser.waitUntil(async () => (await text()).includes("fine"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const fine = '//li[@role="treeitem"][@aria-level="1"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "fine")]]';
      await browser.$('[role="tree"]').click();
      await browser.$(fine).click();
      // A finished Artifact is folded: unfold it, then its eval is the row after it.
      await press(browser, ["l"], async () => (await browser.$(fine).getAttribute("aria-expanded")) === "true", "fine unfolded");
      const check = await browser.$(`${fine}/following-sibling::li[@role="treeitem"][1]`);
      await check.click();
      await press(browser, ["Enter"], () => browser.$("button*=Close").isExisting(), "the Detail opened");
      const request = requests(fixture).find((saved) => saved.evalId === "fine/check");
      assert.ok(request?.result, "the real CLI saved the completed fine/check result");
      assert.equal(request.status, "GREEN");
      assert.equal(request.result.verdict, "GREEN");
      // Read only the opened Detail, not GREEN text already present in the Run tree.
      const detailText = () => browser.$('[role="document"]').getText();
      const field = (name: string) => browser.execute((key) => [...document.querySelectorAll('[role="document"] dt')]
        .find((entry) => entry.textContent === key)?.nextElementSibling?.textContent ?? "", name);
      await browser.waitUntil(async () => {
        if (await field("Request") !== request.id) return false;
        const raw = await field("Raw result");
        return raw !== "" && isDeepStrictEqual(JSON.parse(raw), request.result);
      }, { timeout: 15000, timeoutMsg: "the selected request and its completed result are rendered in Detail" });
      const shown = await detailText();
      assert.match(shown, /Pass\./, `the instruction in ${shown.slice(0, 1500)}`);
      assert.match(shown, /Source\s+executed in this Run/, "where the result came from");
      assert.match(await field("Result"), new RegExp(`^verdict ${request.result.verdict}(?:\n|$)`), "the selected eval's saved verdict");
      assert.deepEqual(JSON.parse(await field("Raw result")), request.result, "Detail renders this eval's exact saved result");
      assert.equal(await field("Request"), request.id, "the opened Detail identifies its exact saved request by default");
      assert.equal(await field("Fingerprint"), "", "additional technical fields are folded initially");
      const technical = () => browser.$("button*=Technical").getAttribute("aria-expanded");
      await press(browser, ["t"], async () => (await technical()) === "true" && (await field("Fingerprint")) !== "" && (await field("Key")) !== "", "the additional technical record is actually rendered");
      assert.equal(await field("Request"), request.id, "showing additional technical fields preserves the default request");
      await press(browser, ["t"], async () => (await technical()) === "false" && (await field("Fingerprint")) === "" && (await field("Key")) === "", "the additional technical record is hidden");
      assert.equal(await field("Request"), request.id, "hiding technical fields keeps the request visible");
      assert.deepEqual(JSON.parse(await field("Raw result")), request.result, "hiding technical fields keeps the completed result visible");
    },
  ),

  // UX edges: E-MON-BLOCKER
  scenario<Record<string, never>>(
    "states-blocked-eval",
    "jumps from the eval blocked by a RED Artifact to that Artifact with b",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("down"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const down = '//li[@role="treeitem"][@aria-level="1"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "down")]]';
      // `down` waits on `red`, which failed: its eval is the row after it.
      await browser.$('[role="tree"]').click();
      await browser.$(`${down}/following-sibling::li[@role="treeitem"][1]`).click();
      const selected = () => browser.$('[role="treeitem"][aria-selected="true"]');
      assert.match(await selected().getText(), /blocked by red/);
      await press(
        browser,
        ["b"],
        async () => (await selected().getAttribute("aria-level")) === "1" && /\bred\b/.test(await selected().getText()),
        "the RED Artifact selected",
      );
    },
  ),
];
