// The Run graph with every kind of verdict, and the Detail of a finished eval: node and edge
// colours for RED, ERROR and blocked Artifacts, each eval's verdict mark and colour, and the
// status, instruction and result of a completed eval's Summary, with the rest under Debug.
import assert from "node:assert/strict";
import { createStates, requests } from "../fixture.ts";
import { press } from "../keys.ts";
import { aside, detail, fact, graphShown, openArtifact, openTab, summary } from "../graph-view.ts";
import { MAIN } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { graphAppearance, graphHasColours, type VerdictLight } from "./graph-appearance.ts";

// Long names are ordinary authored declarations and force the real headline to overflow.
const ATTENTION_SUFFIX = "-with-a-long-name-for-clipped-run-attention-entries";

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
      // Each edge carries what its source passes on: it is drawn in that colour when the source
      // holds others up (RED, ERROR or blocked), else neutral.
      const expectedEdges = {
        "a:red>a:down": "red", "a:crash>a:after": "error", "a:gate>a:later": "active",
        "a:down>a:past": "blocked", "a:later>a:beyond": "waiting",
      } satisfies Record<string, VerdictLight>;
      await browser.waitUntil(async () => graphHasColours(await lights(), expectedNodes, expectedEdges), {
        timeout: 15000, timeoutMsg: "every node's mark and strip in its verdict's own colour, and each relation's stroke neutral or its holding source's colour",
      });
      const { nodes, edges } = await lights();
      assert.deepEqual(Object.fromEntries(Object.entries(nodes).map(([id, node]) => [id, node.light])), expectedNodes);
      assert.deepEqual(Object.fromEntries(Object.entries(edges).map(([id, edge]) => [id, edge.light])), expectedEdges);
      // No legend on the graph: Help pairs each mark with its word.
      assert.equal(await browser.$('[aria-label="Legend"]').isExisting(), false, "no legend on the graph");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL
  scenario<Record<string, never>>(
    "states-finished-detail",
    "shows an eval's status, instruction and result in its Summary, and every other fact under Debug only on demand",
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
      await browser.waitUntil(async () => (await summary(browser)).instruction === "Pass.", { timeout: 15000, timeoutMsg: "fine/check's Summary rendered" });
      const shown = await summary(browser);
      assert.match(shown.status, /^GREEN\b/, `its status first: ${shown.status}`);
      assert.equal(shown.mark, "green", "led by its status's mark");
      assert.ok(shown.blocks.includes("Result"), `its result: ${shown.blocks}`);
      assert.ok(shown.blocks.includes("Fingerprint"), `its fingerprint: ${shown.blocks}`);
      // The rest is folded under Debug: the request, its source and its raw result.
      assert.equal(shown.debug, false, "Debug starts folded");
      assert.equal(await fact(browser, "Request"), "", "the request is a Debug fact");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await summary(browser)).debug === true && (await fact(browser, "Request")) === request.id, "Debug unfolded by t");
      assert.match(await fact(browser, "Source"), /executed in this Run/, "where the result came from");
      assert.deepEqual(JSON.parse(await fact(browser, "Raw result")), request.result, "Detail renders this eval's exact saved result");
      await press(browser, ["t"], async () => (await summary(browser)).debug === false && (await fact(browser, "Request")) === "", "Debug folded again by t");
      assert.equal((await summary(browser)).instruction, "Pass.", "folding Debug keeps the Summary");
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "states-detail-tabs",
    "shows each eval's verdict mark and colour in the Artifact tab's list and on its chip",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:down");
      // Each Artifact's one eval, in the colour of its own verdict. Each Detail closes before the
      // next Artifact's click, so it covers none of it.
      const expected = [
        ["a:red", "red", /RED/, "red", "RED", "check"],
        ["a:crash", "crash", /ERROR/, "error", "ERROR", "check"],
        ["a:down", "down", /blocked by red/, "blocked", "BLOCKED", "check"],
        ["a:gate", "gate", /Human/, "human", "DEFERRED", "signoff"],
        ["a:fine", "fine", /GREEN/, "green", "GREEN", "check"],
      ] as const;
      const painted = new Set<string>();
      const chips = new Map<string, string>();
      // The graph as drawn: each node's mark in its status's colour, its face and outline neutral.
      const look = await graphAppearance(browser);
      for (const [id] of expected) {
        assert.equal(look.nodes[id].border, look.palette["--line-node"], `${id}'s outline is neutral`);
        assert.equal(look.nodes[id].face, look.palette["--surface-panel"], `${id}'s face is neutral`);
      }
      for (const [id, name, status, glyph, shownStatus, evaluation] of expected) {
        const shown = await openArtifact(browser, id, `Artifact ${name}`);
        assert.match(shown.evals[0]?.status ?? "", status, `${name}'s Artifact tab lists its eval's verdict`);
        const row = await browser.execute(() => {
          const glyph = document.querySelector<HTMLElement>('aside[aria-label="Detail"] .evals button .glyph');
          return glyph && glyph.getClientRects().length > 0
            ? { glyph: glyph.querySelector("svg")?.getAttribute("data-icon"), colour: getComputedStyle(glyph).color, ink: getComputedStyle(document.body).color }
            : null;
        });
        assert.ok(row, `the visible ${shownStatus} eval row independently identifies its status colour`);
        assert.equal(row.glyph, glyph, `${name}'s eval row shows its exact verdict mark`);
        // Its tab's chips: the Artifact's, then its one eval's in the row's mark and colour.
        await openTab(browser, `e:${name}/${evaluation}`, { row: true });
        const tabbed = (await detail(browser))!;
        assert.equal(tabbed.tabs.length, 2, `${name}'s Artifact chip and its one eval's chip`);
        assert.equal(tabbed.tabs[1].glyph, glyph, `${name}'s eval chip shows its exact verdict mark`);
        assert.ok(tabbed.tabs[1].painted, `${name}'s mark actually paints`);
        assert.equal(tabbed.tabs[1].colour, row.colour, `${name}'s chip is painted in the same colour as its visible verdict row`);
        assert.notEqual(tabbed.tabs[1].colour, row.ink, `${name}'s mark is not uncoloured body text`);
        chips.set(name, tabbed.tabs[1].colour);
        // An Artifact whose one eval ended shows that eval's verdict: its node in the row's colour.
        if (name !== "gate") assert.equal(look.nodes[id].mark, row.colour, `${name}'s node is painted in its eval row's colour`);
        painted.add(tabbed.tabs[1].colour);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, `${name}'s Detail closed`);
      }
      assert.equal(painted.size, expected.length, "the five status marks have distinct actual colours");
      // The Pane's waiting sign-off shows its mark in the same colour as gate's sign-off chip.
      const waitingMark = await browser.execute(() => {
        const glyph = document.querySelector('[aria-label="Waiting reviews"] [role="option"] .glyph');
        return glyph ? getComputedStyle(glyph).color : "";
      });
      assert.equal(waitingMark, chips.get("gate"), "the waiting review's mark in the colour of gate's sign-off chip");
    },
  ),

  // UX journey: J-MAIN-RUN; UX edges: E-MON-RUNS-GRAPH E-MON-DETAIL-RUN E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "states-run-outcome",
    "names a Run with RED and ERROR evals and a waiting sign-off in its one header row, and opens its outcome from there: its status, the evals that failed, its tally, its time and what it waits for",
    async () => {
      const fixture = await createStates(ATTENTION_SUFFIX);
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.setWindowSize(1040, 760);
      await graphShown(browser, "a:crash");
      /** The header row: the Run's ID, then its headline's verdict and each eval that needs attention, by its mark. */
      const header = () => browser.execute((main) => {
        const row = document.querySelector(`${main} > header`)!;
        const headline = row.querySelector<HTMLElement>("button.headline")!;
        const items = [...row.querySelectorAll("h2, button.headline .status, button.headline .need")].map((item) => item.getBoundingClientRect());
        const rowBox = row.getBoundingClientRect();
        return {
          title: row.querySelector("h2")?.textContent?.trim() ?? "",
          verdict: [headline?.querySelector(".status svg")?.getAttribute("data-icon") ?? "", headline?.querySelector(".status")?.textContent?.trim() ?? ""],
          singleRow: items.every((item) => item.top >= rowBox.top && item.bottom <= rowBox.bottom) &&
            Math.max(...items.map((item) => item.top)) < Math.min(...items.map((item) => item.bottom)),
          clipped: ["hidden", "clip"].includes(getComputedStyle(headline).overflowX) && headline.scrollWidth > headline.clientWidth,
          needs: [...(headline?.querySelectorAll(".need") ?? [])].map((need) => [need.querySelector("svg")?.getAttribute("data-icon") ?? "", need.textContent?.trim() ?? ""]),
        };
      }, MAIN);
      let shown = await header();
      await browser.waitUntil(async () => (shown = await header()).needs.length === 3, { timeout: 15000, timeoutMsg: "the Run's headline drawn" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(shown)}`); });
      assert.match(shown.title, /^run-/, "the Run's ID first");
      assert.deepEqual(shown.needs.sort(), [["error", `crash/check${ATTENTION_SUFFIX}`], ["human", `gate/signoff${ATTENTION_SUFFIX}`], ["red", `red/check${ATTENTION_SUFFIX}`]], JSON.stringify(shown));
      assert.ok(shown.singleRow && shown.clipped, "ID, verdict and attention entries share one clipped row");

      // A click on the headline opens the Run's outcome and what it waits for, both unfolded.
      await browser.$(`${MAIN} > header button.headline`).click();
      await browser.waitUntil(async () => (await detail(browser))?.title === `Run ${shown.title}`, { timeout: 15000, timeoutMsg: "the Run's Detail" });
      const outcome = () => browser.execute(() => {
        const dock = document.querySelector('aside[aria-label="Detail"]')!;
        const facts = Object.fromEntries([...dock.querySelectorAll('section[aria-label="Outcome"] dt')]
          .map((term) => [term.textContent?.trim() ?? "", term.nextElementSibling as HTMLElement]));
        const marks = (dd: HTMLElement | undefined) => [...(dd?.querySelectorAll(":scope > span") ?? [])]
          .map((part) => [part.querySelector("svg")?.getAttribute("data-icon") ?? "", part.textContent?.trim() ?? ""]);
        return {
          status: facts.Status?.textContent?.trim() ?? "",
          statusIcon: facts.Status?.querySelector("svg")?.getAttribute("data-icon") ?? "",
          failed: marks(facts.Failed),
          tally: marks(facts.Tally),
          time: (facts["Running for"] ?? facts.Took)?.textContent?.trim() ?? "",
          waits: [...dock.querySelectorAll('section[aria-label="Waits for"] .body li')].map((item) => item.textContent ?? ""),
          open: dock.querySelector('section[aria-label="Waits for"] button.head')?.getAttribute("aria-expanded"),
        };
      });
      let read = await outcome();
      await browser.waitUntil(async () => (read = await outcome()).status !== "", { timeout: 15000, timeoutMsg: "the Run's outcome" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(read)}`); });
      assert.equal(read.statusIcon, shown.verdict[0], `its status, said once, by the headline's mark: ${read.status}`);
      assert.deepEqual(read.failed, [["red", `red/check${ATTENTION_SUFFIX}`], ["error", `crash/check${ATTENTION_SUFFIX}`]], "the evals that failed, by name");
      assert.equal(read.status, "WAITING HUMAN", "its status in words");
      // How many evals ended in each status, as the fixture's nine Artifacts leave them.
      assert.deepEqual(read.tally.map(([, text]) => text),
        ["1 DEFERRED", "2 PROVISIONAL", "1 GREEN", "1 RED", "1 ERROR", "1 WAIT DEPENDENCY", "2 BLOCKED"], JSON.stringify(read.tally));
      for (const [icon, word] of [["green", "GREEN"], ["red", "RED"], ["error", "ERROR"], ["blocked", "BLOCKED"]]) {
        assert.ok(read.tally.some(([shownIcon, text]) => shownIcon === icon && text.endsWith(` ${word}`)), `the tally marks ${word} with its icon: ${JSON.stringify(read.tally)}`);
      }
      assert.match(read.time, /\d/, "what the Run took so far");
      assert.equal(read.open, "true", "Waits for opens unfolded");
      assert.ok(read.waits.some((item) => item.includes(`gate/signoff${ATTENTION_SUFFIX}`)), "the waiting sign-off is under Waits for");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Run's Detail closed");
    },
  ),
];
