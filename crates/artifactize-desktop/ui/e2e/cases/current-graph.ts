// A branch's Current: its worktree as it is now, scanned as `artifactize status` does, its
// graph marking each Artifact by what verify would do, the Detail of one with its evals' plans
// and its own Human tools, the structure text, and the scans a Refresh, a choice and the end
// of a Run start.
import assert from "node:assert/strict";
import { changeNotes, create, createEvals, createGraph, createMixed, ended, requests, savedRuns, signOff, signoff, verifyWaiting } from "../fixture.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { command, runtimeProfile, script } from "../native-fixtures.ts";
import { makeDirectory, nativePath, primaryModifier, readText, removeTree, writeText } from "../platform.ts";
import { camera, detail, graphShown, nodeAt, openArtifact, openTab, summary } from "../graph-view.ts";
import { branch, chooseChip, mainTitle, scanned, scannedAt } from "../pane.ts";
import { runOf } from "../tool-log.ts";
import { currentHasColours, graphAppearance } from "./graph-appearance.ts";

const CURRENT = 'section[aria-label="Current"]';
const DETAIL = 'aside[aria-label="Detail"]';

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

function current(browser: WebdriverIO.Browser) {
  const structure = () => browser.$(`${CURRENT} pre`).getText();
  const expanded = () => browser.execute((root) => document.querySelector(`${root} details`)?.hasAttribute("open") ?? false, CURRENT);
  /** Each drawn Artifact as read aloud, and each edge's light. */
  const drawn = () => browser.execute(() => ({
    nodes: Object.fromEntries([...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].filter((node) => !node.closest('aside'))
      .map((node) => [node.dataset.id ?? "", node.getAttribute("aria-label") ?? ""])),
    edges: Object.fromEntries([...document.querySelectorAll<SVGElement>(".svelte-flow__edge")].filter((edge) => !edge.closest('aside'))
      .map((edge) => [edge.dataset.id ?? "", edge.dataset.light ?? ""])),
  }));

  /** The camera may have just moved (Fit View). Wait for a stable, hit-testable real pointer
   * target before one WebDriver click, rather than clicking during that transition. */
  async function clickArtifact(name: string) {
    const selector = `${CURRENT} .svelte-flow__node[data-id="a:${name}"]`;
    await browser.waitUntil(() => browser.executeAsync<boolean, [string]>((selector, done) => {
      const node = document.querySelector(selector);
      if (!node) return done(false);
      const before = node.getBoundingClientRect();
      requestAnimationFrame(() => {
        const after = node.getBoundingClientRect();
        const canvas = node.closest(".svelte-flow")?.getBoundingClientRect();
        const hit = document.elementFromPoint(after.x + after.width / 2, after.y + after.height / 2);
        done(node.isConnected && before.x === after.x && before.y === after.y && before.width === after.width && before.height === after.height &&
          !!canvas && after.left >= canvas.left && after.right <= canvas.right && after.top >= canvas.top && after.bottom <= canvas.bottom &&
          hit?.closest(".svelte-flow__node") === node);
      });
    }, selector), { timeout: 15000, timeoutMsg: `${name} is a stable, visible graph pointer target` });
    await browser.$(selector).click();
  }
  return { structure, expanded, drawn, clickArtifact };
}

/** The reason a Current's eval gives for reusing a result kept on this machine. */
const CACHED = "The current eval and fingerprints have a completed cached result.";

/** The text of the open Detail's Summary block titled `title`. */
const block = (browser: WebdriverIO.Browser, title: string) => browser.execute((wanted) => [...document.querySelectorAll('aside[aria-label="Detail"] [role="document"] .block')]
  .find((shown) => shown.querySelector("h3")?.textContent?.trim() === wanted)?.textContent ?? "", title);

/** Each eval row of the open Detail's Artifact tab: its name and what it says. */
const evalRows = (browser: WebdriverIO.Browser) => browser.execute((root) => [...document.querySelectorAll<HTMLElement>(`${root} button[data-eval]`)]
  .map((row) => ({ name: row.querySelector(".name")?.firstChild?.textContent?.trim() ?? "", status: row.querySelector(".status")?.textContent?.trim() ?? "" })), DETAIL);

export const scenarios: Scenario[] = [
  // UX journeys: J-LAUNCH J-MAIN-CURRENT J-DETAIL J-DETAIL-SUMMARY J-DETAIL-TOOLS J-GRAPH-COLOURS J-GRAPH-CAMERA
  // UX edges: E-CURRENT-LAUNCH E-CURRENT-EVALS E-CURRENT-TAB E-DETAIL-TOOL E-CURRENT-STRUCTURE E-CURRENT-CLOSE E-CURRENT-REFRESH E-GRAPH-CAMERA E-GRAPH-CAMERA-KEPT
  scenario<Record<string, never>>(
    "current-launch",
    "opens graph --repo on the branch's Current without a Run, marks what verify would run, opens an Artifact's evals and tools, expands the criteria, closes, then refreshes changed declarations",
    async () => {
      const fixture = createGraph();
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { structure, expanded, drawn, clickArtifact } = current(browser);
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current of the branch of --repo");
      assert.equal((await branch(browser, "repo  master"))?.selected, true, "its branch selected");
      await waitFor(browser, async () => /^Scanned at \d/.test(await scanned(browser)), "its header says when it was scanned");
      await graphShown(browser, "a:release");
      // Nothing was ever verified: what has no inputs would run, what reads release waits.
      await waitFor(browser, async () => JSON.stringify((await drawn()).nodes) === JSON.stringify({
        "a:docs": "docs: would run (1 eval)", "a:release": "release: would run (1 eval)", "a:ship": "ship: waiting",
      }), `each Artifact marked by what verify would do: ${JSON.stringify(await drawn())}`);
      await waitFor(browser, async () => JSON.stringify((await drawn()).edges) === JSON.stringify({ "a:release>a:ship": "run" }),
        `the relation carries what its source would do: ${JSON.stringify(await drawn())}`);
      const appearance = await graphAppearance(browser);
      assert.notEqual(appearance.nodes["a:release"].dot, appearance.nodes["a:ship"].dot, "would run and waiting are painted apart");
      assert.deepEqual(await browser.execute((root) => [...document.querySelectorAll(`${root} [aria-label="Legend"] li`)].map((item) => item.textContent?.trim()), CURRENT),
        ["reused", "would run", "waiting", "blocked by upstream"], "its own legend");
      assert.deepEqual(savedRuns(fixture), [], "scanning saved no Run");

      // A click on release opens its Detail over the Current: its eval would run, and its tools.
      const opened = await openArtifact(browser, "a:release", "Artifact release");
      assert.deepEqual(opened.tabs.map((tab) => tab.name), ["release", "signoff"]);
      assert.ok(opened.docked, "the Detail floats over the Current's graph, docked right with a gap");
      assert.deepEqual(await evalRows(browser), [{ name: "signoff", status: "would run" }]);
      const tools = () => browser.$(`${DETAIL} section[aria-label="Tools of release"]`);
      await tools().$("button=▶ notes").waitForEnabled({ timeout: 15000, timeoutMsg: "release's own Human tools listed" });
      assert.equal(await browser.$(`${CURRENT} .svelte-flow__node button`).isExisting(), false, "the graph's nodes carry no tool buttons");
      // A tool runs at once in the worktree as it is now, with no claim and no review.
      await tools().$("button=▶ notes").click();
      await waitFor(browser, async () => (await runOf(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", "the notes run ended in the Artifact's Tool log");
      assert.deepEqual(await runOf(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      assert.deepEqual(requests(fixture), [], "running a tool asked for no request and recorded no evidence");
      // The eval's tab says it would run and why.
      await openTab(browser, "e:release/signoff");
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("○ would run"), `the eval's plan: ${JSON.stringify(await summary(browser))}`);
      assert.equal((await summary(browser)).status, "○ would run Record a request awaiting a Human claim and submission.", "with why: a Human eval asks a person");
      assert.equal((await summary(browser)).instruction, "Read the release notes and approve them.");
      assert.equal(await browser.$('[aria-label="Human review"]').isExisting(), false, "a Current holds no Human review");

      // Expanding the structure shows every criterion and labeled relation as text.
      assert.equal(await expanded(), false, "the structure text starts folded");
      await browser.$(`${CURRENT} details summary`).click();
      await waitFor(browser, expanded, "structure text expanded");
      const shown = await structure();
      const criteria: Record<string, unknown> = {};
      let evalId = "";
      for (const line of shown.split("\n")) {
        const heading = /├─ ([^:]+):/.exec(line);
        if (heading) evalId = heading[1];
        const payload = /criteria: (\{.*\})$/.exec(line);
        if (payload) criteria[evalId] = JSON.parse(payload[1]);
      }
      assert.deepEqual(criteria, {
        "release/signoff": {
          target: "release", title: "A person approves the release", gate: null,
          criteria: { kind: "human" }, payload: { instruction: "Read the release notes and approve them." },
          passSchema: { type: "object", properties: { approved: { const: true },
            comment: { type: "string", minLength: 1, description: "What you checked." } },
            required: ["approved", "comment"], additionalProperties: false },
          failSchema: { type: "object", properties: { reason: { type: "string", minLength: 3 },
            findings: { type: "array", items: { type: "object", properties: { problem: { type: "string", minLength: 1 }, severity: { enum: ["low", "high"] } },
              required: ["problem"], additionalProperties: false } },
            steps: { type: "array", items: { type: "string" } } },
            required: ["reason"], additionalProperties: false },
        },
        "ship/check": { target: "ship", title: "Ship the release", gate: null,
          criteria: { kind: "runtime", command, args: [script, "pass"] },
          payload: { instruction: "Ship {release}." }, passSchema: null, failSchema: null },
        "docs/lint": { target: "docs", title: "Docs build", gate: null,
          criteria: { kind: "runtime", command, args: [script, "pass"] },
          payload: { instruction: "Build the docs." }, passSchema: null, failSchema: null },
      }, "every independently declared criterion and owner schema is exposed");
      assert.deepEqual(shown.split("\n").filter((line) => line.includes(" → ")), [
        'release → ship {"kind":"instruction","eval_id":"ship/check","name":"release"}',
      ], "the concrete input-to-consumer relation carries its instruction label");

      // Esc closes the Detail; Close does too, and a click on the background. The expanded text
      // left the graph less room without moving its camera: folded again, with Fit View, the
      // graph shows whole.
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closed release's Detail");
      await browser.$(`${CURRENT} details summary`).click();
      await waitFor(browser, async () => !(await expanded()), "structure text folded");
      await browser.$(`${CURRENT} .svelte-flow__controls-fitview`).click();
      await camera(browser);
      await clickArtifact("release");
      await waitFor(browser, async () => (await detail(browser)) !== null, "release's Detail open again");
      await browser.$(DETAIL).$("button*=Close").click();
      await waitFor(browser, async () => (await detail(browser)) === null, "Close closed release's Detail");
      await clickArtifact("docs");
      await waitFor(browser, async () => (await detail(browser))?.title === "Artifact docs", "docs' Detail open");
      const background = await browser.execute((root) => {
        const canvas = document.querySelector(`${root} .canvas`)!.getBoundingClientRect();
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        const nodes = [...document.querySelectorAll(`${root} .svelte-flow__node`)].map((node) => node.getBoundingClientRect());
        const below = Math.max(...nodes.map((node) => node.bottom));
        return { x: Math.round((canvas.left + dock.left) / 2), y: Math.round(Math.min(canvas.bottom - 20, below + (canvas.bottom - below) / 2)) };
      }, CURRENT);
      await browser.action("pointer").move(background).down().up().perform();
      await waitFor(browser, async () => (await detail(browser)) === null, "a click on the background closed docs' Detail");

      // Each Refresh, Ctrl+R and F5 scans the changed declarations again, the text staying open.
      await browser.$(`${CURRENT} details summary`).click();
      await waitFor(browser, expanded, "structure text expanded again");
      // The person zooms out; no scan or new layout below moves that camera.
      await browser.$(`${CURRENT} .svelte-flow__controls-zoomout`).click();
      const moved = await camera(browser);
      const declaration = nativePath(fixture.repo, "docs", "index.artf");
      let previousTitle = "Docs build";
      for (const [index, keys] of [null, [primaryModifier, "r"], ["F5"]].entries()) {
        const title = `Current docs criterion ${index}`;
        writeText(declaration, readText(declaration).replace(/title = "[^"]*"/, `title = "${title}"`));
        assert.doesNotMatch(await structure(), new RegExp(title), "the change shows only after a scan");
        if (keys) await press(browser, keys, async () => (await structure()).includes(title), `scanned again with ${keys.join("+")}`);
        else {
          await browser.$(CURRENT).$("button=Refresh").click();
          await waitFor(browser, async () => (await structure()).includes(title), "scanned again with Refresh");
        }
        assert.ok(!(await structure()).includes(previousTitle), "the structure text replaced the previous declaration title");
        assert.equal(await expanded(), true, "the structure text stays open");
        previousTitle = title;
      }
      assert.equal(await camera(browser), moved, "the scans left the person's camera");
      // A scan also redraws the structure: an Artifact that reads docs adds a node and an edge,
      // and removing it takes both away.
      const digest = makeDirectory(nativePath(fixture.repo, "digest"));
      writeText(nativePath(digest, "index.artf"), `name = "digest"

[evals.summary]
title = "Summarize the docs"
profile = ${runtimeProfile()}

[evals.summary.payload]
instruction = """
Summarize {docs}."""
`);
      await browser.$(CURRENT).$("button=Refresh").click();
      await waitFor(browser, async () => {
        const { nodes, edges } = await drawn();
        return nodes["a:digest"] === "digest: waiting" && edges["a:docs>a:digest"] !== undefined;
      }, "the added digest Artifact and its relation from docs drawn");
      assert.ok((await structure()).split("\n").some((line) => line.startsWith("docs → digest ")), "the structure text lists the added relation");
      assert.equal(await camera(browser), moved, "the new layout left the person's camera");
      await browser.$(CURRENT).$("button=Refresh").waitForEnabled({ timeout: 15000 });
      removeTree(digest);
      await browser.$(CURRENT).$("button=Refresh").click();
      await waitFor(browser, async () => (await drawn()).nodes["a:digest"] === undefined, "the removed digest Artifact dropped");
      assert.ok(!(await structure()).includes("digest"), "the structure text drops the removed Artifact");
      // Only now a real Run in this same state: the probe that found no Run would have seen one.
      const controlRun = await verifyWaiting(fixture);
      assert.deepEqual(savedRuns(fixture).map((run) => run.id), [controlRun], "the same saved-Run probe sees this fixture's real Run");
    },
  ),

  // UX journeys: J-DETAIL-SUMMARY J-GRAPH-COLOURS; UX edges: E-CURRENT-CHIP E-CURRENT-EVALS E-CURRENT-TAB
  scenario<{ run: string }>(
    "current-reused",
    "marks an Artifact whose every eval would be reused, and its eval's tab says why with the result it would reuse",
    async () => {
      const fixture = createEvals(["alpha", "beta"]);
      return { fixture, args: ["--repo", fixture.repo], data: { run: savedRuns(fixture)[0].id } };
    },
    async (browser, { data: { run } }) => {
      // The Run ended, so the branch's newest entry is its Current.
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown");
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => [chip.id, chip.filled]), [["current", true], [run, false]]);
      await graphShown(browser, "a:app");
      await waitFor(browser, async () => (await browser.$(nodeAt("a:app")).getAttribute("aria-label")) === "app: reused", "app marked reused");
      const opened = await openArtifact(browser, "a:app", "Artifact app");
      assert.deepEqual(opened.tabs.slice(1).map((tab) => [tab.name, tab.glyph]), [["alpha", "↻"], ["beta", "↻"]], "each eval's chip says it would be reused");
      assert.deepEqual(await evalRows(browser), [{ name: "alpha", status: "reused" }, { name: "beta", status: "reused" }]);
      assert.equal(await browser.$(`${DETAIL} section[aria-label="Tools of app"]`).isExisting(), true, "its Tools section, here without tools");
      await openTab(browser, "e:app/beta", { row: true });
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("↻ reused"), `beta's plan: ${JSON.stringify(await summary(browser))}`);
      const shown = await summary(browser);
      assert.ok(shown.blocks.includes("Result"), `the result it would reuse: ${shown.blocks}`);
      assert.equal(shown.status, `↻ reused ${CACHED}`, "with why");
      assert.equal(shown.instruction, "Check beta.");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
    },
  ),

  // UX journeys: J-MAIN-CURRENT J-DETAIL-SUMMARY J-GRAPH-COLOURS
  // UX edges: E-CURRENT-CHIP E-CURRENT-EVALS E-CURRENT-TAB E-CURRENT-RUN-END E-GRAPH-RECOLOUR
  scenario<{ first: string }>(
    "current-run-end",
    "shows the signed-off result a Current would reuse, scans again when its chip is chosen again and when a Run of its branch ends, recolouring the graph in place",
    async () => {
      const fixture = await create();
      const first = signoff(fixture);
      signOff(fixture, first.id, '{"approved":true,"comment":"Checked notes 7f3a against the tag."}');
      // The signed-off Run has ended before the app starts: the branch opens on its Current.
      assert.equal(ended(fixture, first.runId), "GREEN", "the first Run ended GREEN");
      return { fixture, args: ["--repo", fixture.repo], data: { first: first.runId } };
    },
    async (browser, { fixture }) => {
      const { drawn } = current(browser);
      const placed = () => browser.execute(() => Object.fromEntries([...document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node')]
        .map((node) => [node.dataset.id ?? "", node.style.transform])));
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown");
      const allReused = { "a:docs": "docs: reused", "a:release": "release: reused", "a:ship": "ship: reused" };
      await waitFor(browser, async () => JSON.stringify((await drawn()).nodes) === JSON.stringify(allReused), `every Artifact reused: ${JSON.stringify(await drawn())}`);
      const at = await placed();
      // The sign-off's tab holds the result it would reuse, its owner fields by name, and why.
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("↻ reused"), `the sign-off's plan: ${JSON.stringify(await summary(browser))}`);
      assert.equal((await summary(browser)).status, `↻ reused ${CACHED}`);
      const result = await block(browser, "Result");
      assert.match(result, /comment.*Checked notes 7f3a against the tag\./s, `the reused comment: ${result}`);
      assert.match(result, /approved.*true/s, `the reused approval: ${result}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      const reusedLook = await graphAppearance(browser);
      // The notes change on disk; the Current shows it only once its chip is chosen again.
      changeNotes(fixture, "# Release notes\n- A second release.\n");
      assert.deepEqual((await drawn()).nodes, allReused, "no scan without a choice, a Refresh or a Run's end");
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await drawn()).nodes["a:release"] === "release: would run (1 eval)", "release would run again");
      assert.equal((await drawn()).nodes["a:ship"], "ship: waiting", "ship waits for it");
      /** Scans the app has completed, from its read-only command activity. */
      const scans = async () => (await checkpoint(browser)).activity.commands.currentScan?.completed ?? 0;
      const before = { checkpoint: await checkpoint(browser), scans: await scans(), at: await scannedAt(browser), drawn: await drawn(), look: await graphAppearance(browser) };
      assert.ok(before.at, "the header names the scan before the Run");
      assert.notEqual(before.look.nodes["a:release"].dot, reusedLook.nodes["a:release"].dot, "would run is painted unlike reused");
      assert.ok(currentHasColours(reusedLook, { "a:docs": "reused", "a:release": "reused", "a:ship": "reused" }, { "a:release>a:ship": "reused" }),
        `reused in the legend's reused colour: ${JSON.stringify(reusedLook)}`);
      assert.ok(currentHasColours(before.look, { "a:release": "run", "a:ship": "waiting" }, { "a:release>a:ship": "run" }),
        `would run and waiting in their legend colours: ${JSON.stringify(before.look)}`);
      // Another process verifies: while its Run waits the row starts with it; signed off, the
      // Run ends and the Current is scanned again by itself.
      const run = await verifyWaiting(fixture);
      await waitFor(browser, async () => (await branch(browser, "repo  master"))?.chips[0]?.id === run, "the Run in progress starts the row");
      assert.equal(await mainTitle(browser), "Current · repo  master", "the Current stays shown");
      const request = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "release/signoff");
      assert.ok(request);
      signOff(fixture, request.id);
      await waitFor(browser, async () => JSON.stringify((await drawn()).nodes) === JSON.stringify(allReused), "the Run's end scanned the Current again: everything reused");
      assert.deepEqual(await placed(), at, "recoloured in place: no Artifact moved");
      // A scan of its own after the one before the Run, drawn in the reused colours again. Its
      // freshness is the completed-scan generation: once that scan has rendered, the header
      // names that scan's own instant, not the one before.
      assert.ok((await scans()) > before.scans, "the Run's end completed a new scan");
      await commandsRendered(browser, before.checkpoint);
      const instant = await scannedAt(browser);
      assert.ok(instant && instant !== before.at, `the header names the new scan, not ${before.at}: ${instant}`);
      const local = await browser.execute((shown: string) => new Date(shown).toLocaleTimeString(), instant);
      assert.equal(await scanned(browser), `Scanned at ${local}`, "the header reads that scan's local time");
      const after = await graphAppearance(browser);
      for (const id of ["a:release", "a:ship"]) {
        assert.notEqual(after.nodes[id].dot, before.look.nodes[id].dot, `${id} recoloured`);
        assert.equal(after.nodes[id].dot, reusedLook.nodes[id].dot, `${id} painted as reused`);
      }
      assert.notEqual((await drawn()).edges["a:release>a:ship"], before.drawn.edges["a:release>a:ship"], "the relation relit by its source's new plan");
      // Its drawn line changes colour with it: from would run back to reused.
      const edge = "a:release>a:ship";
      assert.ok(after.edges[edge].painted, "the relation is drawn");
      assert.notEqual(before.look.edges[edge].stroke, reusedLook.edges[edge].stroke, "would run's relation is stroked unlike reused");
      assert.notEqual(after.edges[edge].stroke, before.look.edges[edge].stroke, "the relation's stroke changed");
      assert.equal(after.edges[edge].stroke, reusedLook.edges[edge].stroke, "the relation stroked as reused");
    },
  ),

  // UX journeys: J-MAIN-CURRENT J-GRAPH-COLOURS; UX edges: E-CURRENT-CHIP E-CURRENT-EVALS
  scenario<Record<string, never>>(
    "current-mixed",
    "marks a Current's Artifacts blocked by a RED one, and an Artifact some of whose evals would run with how many",
    async () => {
      const fixture = createMixed();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { drawn } = current(browser);
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown after the ended Run");
      await graphShown(browser, "a:app");
      const marked = { "a:app": "app: would run (2 evals)", "a:down": "down: blocked", "a:red": "red: reused" };
      await waitFor(browser, async () => JSON.stringify((await drawn()).nodes) === JSON.stringify(marked), `each Artifact marked: ${JSON.stringify(await drawn())}`);
      // Drawn by colour alone, as a Run's Artifacts are: each node shows only its name.
      const texts = await browser.execute(() => Object.fromEntries([...document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node')]
        .filter((node) => !node.closest("aside")).map((node) => [node.dataset.id ?? "", node.textContent?.trim() ?? ""])));
      assert.deepEqual(texts, { "a:app": "app", "a:down": "down", "a:red": "red" });
      const look = await graphAppearance(browser);
      assert.notEqual(look.nodes["a:down"].dot, look.nodes["a:app"].dot, "blocked is painted unlike would run");
      assert.notEqual(look.nodes["a:down"].dot, look.nodes["a:red"].dot, "blocked is painted unlike reused");
      assert.ok(currentHasColours(look, { "a:app": "run", "a:down": "blocked", "a:red": "reused" }, {}),
        `each Artifact in its legend colour: ${JSON.stringify(look)}`);
      // Two of app's three evals would run; the third's result is reused.
      await openArtifact(browser, "a:app", "Artifact app");
      assert.deepEqual(await evalRows(browser), [
        { name: "alpha", status: "reused" }, { name: "beta", status: "would run" }, { name: "gamma", status: "would run" },
      ]);
      // A title wider than the Detail wraps; each eval's name and status stay on one line.
      const lines = await browser.execute(() => [...document.querySelectorAll('aside[aria-label="Detail"] .evals .eval')].map((row) => {
        const count = (part: Element | null) => {
          if (!part) return 0;
          const range = document.createRange();
          range.selectNodeContents(part);
          return new Set([...range.getClientRects()].map((box) => Math.round(box.top))).size;
        };
        return { name: count(row.querySelector(".name")), status: count(row.querySelector(".status")), title: count(row.querySelector(".right")) };
      }));
      assert.ok(lines[0].title > 1, `alpha's long title wraps: ${JSON.stringify(lines)}`);
      assert.ok(lines.every((row) => row.name === 1 && row.status === 1), `names and statuses on one line: ${JSON.stringify(lines)}`);
      await openTab(browser, "e:app/alpha", { row: true });
      await waitFor(browser, async () => (await summary(browser)).status === `↻ reused ${CACHED}`, `alpha's plan: ${JSON.stringify(await summary(browser))}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "app's Detail closed");
      // down cannot be evaluated while red is RED, and says so.
      await openArtifact(browser, "a:down", "Artifact down");
      assert.deepEqual(await evalRows(browser), [{ name: "check", status: "blocked" }]);
      await openTab(browser, "e:down/check", { row: true });
      await waitFor(browser, async () => /^\S+ blocked \S.*red\/check/.test((await summary(browser)).status),
        `down's plan is blocked and names the RED eval that blocks it: ${JSON.stringify(await summary(browser))}`);
    },
  ),
];

