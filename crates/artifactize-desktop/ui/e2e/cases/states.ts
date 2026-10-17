// The Run graph with every kind of verdict, and the Detail of a finished eval: node and edge
// colours for RED, ERROR and blocked Artifacts, each eval tab's verdict glyph and colour, and the
// instruction, request, result and technical record of a completed eval.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { createStates, requests } from "../fixture.ts";
import { press } from "../keys.ts";
import { detail, graphShown, openArtifact, openTab } from "../graph-view.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type VerdictLight } from "./graph-appearance.ts";

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

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL
  scenario<Record<string, never>>(
    "states-finished-detail",
    "shows the instruction, the request and the result, and the technical record only on demand",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:fine");
      // A click on fine opens its Detail; a click on its one eval's tab shows that eval.
      await openArtifact(browser, "a:fine", "Artifact fine");
      await openTab(browser, "e:fine/check");
      const request = requests(fixture).find((saved) => saved.evalId === "fine/check");
      assert.ok(request?.result, "the real CLI saved the completed fine/check result");
      assert.equal(request.status, "GREEN");
      assert.equal(request.result.verdict, "GREEN");
      // Read only the eval's tab, not GREEN text of the graph or the Artifact tab.
      const detailText = () => browser.$('aside[aria-label="Detail"] [role="document"]').getText();
      const field = (name: string) => browser.execute((key) => [...document.querySelectorAll('aside[aria-label="Detail"] [role="document"] dt')]
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
      const technical = () => browser.$('aside[aria-label="Detail"]').$("button*=Technical").getAttribute("aria-expanded");
      await press(browser, ["t"], async () => (await technical()) === "true" && (await field("Fingerprint")) !== "" && (await field("Key")) !== "", "the additional technical record is actually rendered");
      assert.equal(await field("Request"), request.id, "showing additional technical fields preserves the default request");
      await press(browser, ["t"], async () => (await technical()) === "false" && (await field("Fingerprint")) === "" && (await field("Key")) === "", "the additional technical record is hidden");
      assert.equal(await field("Request"), request.id, "hiding technical fields keeps the request visible");
      assert.deepEqual(JSON.parse(await field("Raw result")), request.result, "hiding technical fields keeps the completed result visible");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "states-detail-tabs",
    "shows each eval's verdict glyph and colour on its tab and in the Artifact tab's list",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:down");
      // Each Artifact's one eval, in the colour of its own verdict. Each Detail closes before the
      // next Artifact's click, so it covers none of it.
      const expected = [
        ["a:red", "red", "tone-red", /RED/],
        ["a:crash", "crash", "tone-error", /ERROR/],
        ["a:down", "down", "tone-blocked", /blocked by red/],
        ["a:gate", "gate", "tone-human", /Human/],
        ["a:fine", "fine", "tone-green", /GREEN/],
      ] as const;
      for (const [id, name, tone, status] of expected) {
        const shown = await openArtifact(browser, id, `Artifact ${name}`);
        assert.equal(shown.tabs.length, 2, `${name}'s Artifact tab and its one eval's tab`);
        assert.equal(shown.tabs[1].tone, tone, `${name}'s eval tab shows its verdict's colour`);
        assert.notEqual(shown.tabs[1].glyph, "", `${name}'s eval tab shows its verdict's glyph`);
        assert.match(shown.evals[0]?.status ?? "", status, `${name}'s Artifact tab lists its eval's verdict`);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, `${name}'s Detail closed`);
      }
    },
  ),
];
