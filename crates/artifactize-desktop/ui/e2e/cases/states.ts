// The Run graph with every kind of verdict, and the Detail of a finished eval: node and edge
// colours for RED, ERROR and blocked Artifacts, the arrows between nodes of one column, and the
// instruction, request, result and technical record of a completed eval.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { createStates, requests } from "../fixture.ts";
import { press } from "../keys.ts";
import { focusedNode, graphShown, nodeAt, popup, rowAt, summaryShows } from "../graph-view.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type VerdictLight } from "./graph-appearance.ts";

/** A clicked node pops up its evals. Escape then closes them: it is pressed only once the
 * popup is shown, since before it Escape leaves the Run pane. */
async function popsUp(browser: WebdriverIO.Browser, artifact: string): Promise<void> {
  await browser.waitUntil(async () => (await popup(browser))?.artifact === artifact, { timeout: 15000, timeoutMsg: `${artifact}'s evals popped up` });
}

export const scenarios: Scenario[] = [
  // UX journey J-GRAPH-COLOURS: every verdict's own colour, on nodes and on edges.
  scenario<Record<string, never>>(
    "states-graph-lights",
    "colours RED, ERROR, blocked, waiting and GREEN Artifacts and the edges each passes on to its dependent",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const lights = () => graphAppearance(browser);
      await graphShown(browser, "a:crash");
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
      await graphShown(browser, "a:red");
      const on = (id: string) => async () => (await focusedNode(browser)) === id;
      await browser.$(nodeAt("a:red")).click();
      // A click pops up the evals; close them to move on the whole graph with the node focused.
      await popsUp(browser, "red");
      await press(browser, ["Escape"], async () => (await popup(browser)) === null && await on("a:red")(), "the popup closed on red");
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
      await browser.$(nodeAt(order[0])).click();
      await popsUp(browser, order[0].slice(2));
      await press(browser, ["Escape"], async () => (await popup(browser)) === null && await on(order[0])(), `the first node in the column, ${order[0]}, focused`);
      for (const next of order.slice(1)) {
        await press(browser, ["ArrowDown"], on(next), `focus on ${next}, below`);
        await summaryShows(browser, nodeAt(next), new RegExp(next.slice(2)));
      }
      await press(browser, ["ArrowUp"], on(order[order.length - 2]), `focus on ${order[order.length - 2]}, above`);
    },
  ),

  // UX edges: E-GRAPH-EVALS-OPEN E-GRAPH-EVALS-DETAIL E-MON-DETAIL-TECHNICAL
  scenario<Record<string, never>>(
    "states-finished-detail",
    "shows the instruction, the request and the result, and the technical record only on demand",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:fine");
      await browser.$(nodeAt("a:fine")).click();
      await popsUp(browser, "fine");
      // A click on the popup's one eval opens its Detail.
      await browser.$(rowAt("a:fine/e:fine/check")).click();
      await browser.$('aside[aria-label="Detail"]').waitForExist({ timeout: 15000, timeoutMsg: "the Detail opened" });
      const request = requests(fixture).find((saved) => saved.evalId === "fine/check");
      assert.ok(request?.result, "the real CLI saved the completed fine/check result");
      assert.equal(request.status, "GREEN");
      assert.equal(request.result.verdict, "GREEN");
      // Read only the opened Detail, not GREEN text already present in the popup.
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
      await graphShown(browser, "a:down");
      // `down` waits on `red`, which failed: its one eval is blocked.
      await browser.$(nodeAt("a:down")).click();
      await popsUp(browser, "down");
      const blocked = "a:down/e:down/check";
      await browser.waitUntil(async () => (await popup(browser))?.selected === blocked && (await popup(browser))?.focused === true,
        { timeout: 15000, timeoutMsg: "down's blocked eval selected in its focused popup" });
      assert.match(await browser.$(rowAt(blocked)).getText(), /blocked by red/);
      await summaryShows(browser, rowAt(blocked), /blocked/);
      await press(browser, ["b"], async () => (await popup(browser)) === null && (await focusedNode(browser)) === "a:red", "the RED Artifact focused");
      await summaryShows(browser, nodeAt("a:red"), /red/);
    },
  ),
];
