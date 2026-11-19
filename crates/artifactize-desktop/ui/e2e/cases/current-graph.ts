import { writeDeclaration } from "../declaration-fixtures.ts";
// A branch's Current: its worktree as it is now, scanned as `artifactize status` does, its
// graph marking each Artifact by what verify would do, the Detail of one with its evals' plans
// and its own Human tools, and the scans a Refresh, a choice and the end of a Run start.
import assert from "node:assert/strict";
import {
  artifactize, changeNotes, create, createChecking, createEvals, createGraph, createMixed, ended, fingerprintRuns, requests, savedRuns, signOff, signoff,
  verifyWaiting,
} from "../fixture.ts";
import { git } from "../environment.ts";
import { graphToolFailure, graphToolObservation } from "../graph-tool-session.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { commandFields, runtimeProfile } from "../native-fixtures.ts";
import { exists, makeDirectory, nativePath, portablePath, primaryModifier, readJson, readText, removeTree, writeText, type NativePath } from "../platform.ts";
import { camera, detail, dockedDetail, graphShown, nodeAt, openArtifact, openArtifactTab, openTab, summary } from "../graph-view.ts";
import { branch, chooseChip, MAIN, mainTitle, scanned, scannedAt } from "../pane.ts";
import { runOf } from "../tool-log.ts";
import { currentHasColours, graphAppearance } from "./graph-appearance.ts";

const CURRENT = 'section[aria-label="Current"]';
const DETAIL = 'aside[aria-label="Detail"]';

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The Main area's Refresh, at the end of a Current's header row. */
const refresh = (browser: WebdriverIO.Browser) => browser.$(`${MAIN} > header`).$("button=Refresh");

function current(browser: WebdriverIO.Browser) {
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
    // Empty when it is a target; otherwise which condition failed, with the boxes involved.
    let last = "";
    await browser.waitUntil(async () => {
      last = await browser.executeAsync<string, [string]>((selector, done) => {
        const node = document.querySelector(selector);
        if (!node) return done("no node");
        const before = node.getBoundingClientRect();
        requestAnimationFrame(() => {
          const after = node.getBoundingClientRect();
          const canvas = node.closest(".svelte-flow")?.getBoundingClientRect();
          const hit = document.elementFromPoint(after.x + after.width / 2, after.y + after.height / 2);
          const box = (rect: DOMRect | undefined) => rect ? [rect.left, rect.top, rect.width, rect.height].map(Math.round).join(",") : "none";
          const facts = `node ${box(after)}, canvas ${box(canvas)}, window ${innerWidth}×${innerHeight}`;
          if (!node.isConnected) return done("detached");
          if (before.x !== after.x || before.y !== after.y || before.width !== after.width || before.height !== after.height) return done(`moving: ${facts}`);
          if (!canvas || after.left < canvas.left || after.right > canvas.right || after.top < canvas.top || after.bottom > canvas.bottom) return done(`outside its canvas: ${facts}`);
          if (hit?.closest(".svelte-flow__node") !== node) return done(`covered by ${hit?.tagName ?? "nothing"} ${hit?.getAttribute("class") ?? ""}: ${facts}`);
          done("");
        });
      }, selector);
      return last === "";
    }, { timeout: 15000, timeoutMsg: `${name} is a stable, visible graph pointer target` })
      .catch((error: Error) => { throw new Error(`${error.message}: ${last}`); });
    await browser.$(selector).click();
  }
  return { drawn, clickArtifact };
}

/** The reason a Current's eval gives for reusing a result kept on this machine. */
const CACHED = "The current eval and fingerprints have a completed cached result.";

/** The text of the open Detail's section `name`, below its header row. */
const block = (browser: WebdriverIO.Browser, name: string) => browser.execute((wanted) => [...document.querySelectorAll<HTMLElement>('aside[aria-label="Detail"] [role="document"] .section')]
  .find((shown) => shown.querySelector(".head")?.textContent?.trim() === wanted)?.querySelector(".body")?.textContent ?? "", name);

/** Each eval row of the open Detail's Artifact tab: its name and what it says. */
const evalRows = (browser: WebdriverIO.Browser) => browser.execute((root) => [...document.querySelectorAll<HTMLElement>(`${root} button[data-eval]`)]
  .map((row) => ({ name: row.querySelector(".name")?.firstChild?.textContent?.trim() ?? "", status: row.querySelector(".status")?.textContent?.trim() ?? "" })), DETAIL);

export const scenarios: Scenario[] = [
  // UX journey: J-LAUNCH
  scenario<Record<string, never>>(
    "current-human-graph-tool",
    "opens the selected sibling worktree's Current in a real companion window when the declared Human graph tool is clicked",
    async () => {
      const fixture = createGraph();
      const worktree = nativePath(fixture.root, "graph-worktree");
      git(fixture.repo, "-C", fixture.repo, "add", ".");
      git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Graph tool inputs");
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-qb", "graph-tool", worktree);
      const docs = nativePath(worktree, ".artifactize", "docs.artf");
      writeText(docs, readText(docs).replace("[views.human_tools]", '[views.human_tools]\ngraph = { builtin = "graph", args = [] }'));
      writeDeclaration(worktree, nativePath(worktree, "graph-marker", "index.artf"), `name = "graph-marker"
[evals.check]
title = "Check the sibling worktree's marker"
profile = ${runtimeProfile()}
payload.instruction = "Check {graph-marker}."
`);
      return { fixture, args: ["graph", "--repo", worktree], data: {} };
    },
    async (browser, { fixture }) => {
      const worktree = "repo  graph-tool";
      await waitFor(browser, async () => (await mainTitle(browser)) === `Current · ${worktree}`, "the sibling worktree selected in the parent window");
      await graphShown(browser, "a:docs");
      await openArtifact(browser, "a:docs", "Artifact docs");
      const evidence = graphToolObservation(fixture);
      assert.equal(exists(evidence), false, "no companion window was opened by setup");
      const tool = browser.$(DETAIL).$('section[aria-label="Tools of docs"]').$("button=graph");
      await tool.waitForEnabled();
      await tool.click();
      await waitFor(browser, async () => {
        const failure = graphToolFailure(fixture);
        if (exists(failure)) throw new Error(`graph companion observation failed: ${readText(failure)}`);
        return (await runOf(browser, DETAIL, "graph", "docs"))?.at(-1)?.kind === "end";
      }, "the tool exits after its window is observed and released", 60000);
      assert.equal((await runOf(browser, DETAIL, "graph", "docs"))?.at(-1)?.text, "■ exit 0", JSON.stringify(await runOf(browser, DETAIL, "graph", "docs")));
      await waitFor(browser, () => exists(evidence), "the renderer observation is saved after the actual companion quits");
      const shown = readJson(evidence) as { title: string; selection: string[]; artifacts: string[] };
      assert.equal(shown.title, `Current · ${worktree}`, "the tool-opened window's Main area");
      assert.deepEqual(shown.selection, [worktree], "the sibling branch is selected in the new window");
      assert.ok(shown.artifacts.includes("graph-marker: would run (1 eval)"), `the new window drew the sibling's live-only declaration: ${JSON.stringify(shown)}`);
    },
  ),

  // UX journeys: J-LAUNCH J-MAIN-CURRENT J-DETAIL J-DETAIL-SUMMARY J-DETAIL-TOOLS J-GRAPH-COLOURS J-GRAPH-CAMERA
  scenario<Record<string, never>>(
    "current-launch",
    "opens graph --repo on the branch's Current without a Run, marks what verify would run, opens an Artifact's evals and tools, closes, then refreshes changed declarations",
    async () => {
      const fixture = createGraph();
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { drawn, clickArtifact } = current(browser);
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current of the branch of --repo");
      assert.equal((await branch(browser, "repo  master"))?.selected, true, "its branch selected");
      await waitFor(browser, async () => /^Scanned at \d/.test(await scanned(browser)), "its header says when it was scanned");
      await graphShown(browser, "a:release");
      // Nothing was ever verified: every Artifact would run. ship reads release, whose only eval
      // is a sign-off, and a sign-off holds up nothing.
      await waitFor(browser, async () => JSON.stringify((await drawn()).nodes) === JSON.stringify({
        "a:docs": "docs: would run (1 eval)", "a:release": "release: would run (1 eval)", "a:ship": "ship: would run (1 eval)",
      }), `each Artifact marked by what verify would do: ${JSON.stringify(await drawn())}`);
      await waitFor(browser, async () => JSON.stringify((await drawn()).edges) === JSON.stringify({ "a:release>a:ship": "run" }),
        `the relation carries what its source would do: ${JSON.stringify(await drawn())}`);
      let look = await graphAppearance(browser);
      await browser.waitUntil(async () => currentHasColours(look = await graphAppearance(browser), { "a:release": "run", "a:ship": "run", "a:docs": "run" }, { "a:release>a:ship": "run" }),
        { timeout: 15000, timeoutMsg: "every Artifact painted as would run, its relation neutral" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(look)}`); });
      // No legend: Help pairs each mark with its word.
      assert.equal(await browser.$(`${CURRENT} [aria-label="Legend"]`).isExisting(), false, "no legend on the graph");
      assert.deepEqual(savedRuns(fixture), [], "scanning saved no Run");

      // A click on release opens its Detail over the Current: its eval would run, and its tools.
      await openArtifact(browser, "a:release", "Artifact release");
      await dockedDetail(browser, "Artifact release");
      assert.deepEqual(await evalRows(browser), [{ name: "signoff", status: "would run" }]);
      const tools = () => browser.$(`${DETAIL} section[aria-label="Tools of release"]`);
      await tools().$("button=notes").waitForEnabled({ timeout: 15000, timeoutMsg: "release's own Human tools listed" });
      assert.equal(await browser.$(`${CURRENT} .svelte-flow__node button`).isExisting(), false, "the graph's nodes carry no tool buttons");
      // A tool runs at once in the worktree as it is now, with no claim and no review.
      await tools().$("button=notes").click();
      await waitFor(browser, async () => (await runOf(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", "the notes run ended in the Artifact's Tool log");
      assert.deepEqual(await runOf(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      assert.deepEqual(requests(fixture), [], "running a tool asked for no request and recorded no evidence");
      // The eval's tab, its chips a path back to the Artifact, says it would run and why.
      await openTab(browser, "e:release/signoff");
      assert.deepEqual((await detail(browser))?.tabs.map((tab) => tab.name), ["release", "signoff"]);
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("would run"), `the eval's plan: ${JSON.stringify(await summary(browser))}`);
      assert.equal((await summary(browser)).status, "would run Record a request awaiting a Human claim and submission.", "with why: a Human eval asks a person");
      assert.equal((await summary(browser)).mark, "run", "led by the would-run mark");
      assert.equal((await summary(browser)).instruction, "Read the release notes and approve them.");
      assert.equal(await browser.$('[aria-label="Human review"]').isExisting(), false, "a Current holds no Human review");

      // Esc closes the Detail; Close does too, and a click on the background. With Fit, the
      // graph shows whole.
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "Esc closed release's Detail");
      await browser.$('[aria-label="Graph tools"]').$("button=Fit").click();
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

      // Each Refresh, Ctrl+R and F5 scans the changed declarations again; docs' eval, open in
      // the Detail, shows each new instruction once a scan read it.
      await clickArtifact("docs");
      await openTab(browser, "e:docs/lint");
      await waitFor(browser, async () => (await summary(browser)).instruction === "Build the docs.", "docs' eval open");
      // The person zooms out; no scan or new layout below moves that camera.
      const beforeZoom = await camera(browser);
      await browser.$('[aria-label="Graph tools"] button[aria-label="Zoom out"]').click();
      await waitFor(browser, async () => (await camera(browser)) !== beforeZoom, "Zoom out visibly moved the camera");
      const moved = await camera(browser);
      const declaration = nativePath(fixture.repo, ".artifactize", "docs.artf");
      let previous = "Build the docs.";
      const instruction = async () => (await summary(browser)).instruction;
      /** The last scan ended: Refresh is offered again. */
      const scanEnded = () => refresh(browser).waitForEnabled({ timeout: 15000 });
      // Ctrl+R and F5 come at once, the previous scan maybe still running: what they were
      // pressed for shows all the same.
      for (const [index, keys] of [null, [primaryModifier, "r"], ["F5"]].entries()) {
        const wanted = `Build the docs, take ${index}.`;
        if (!keys) await scanEnded();
        writeText(declaration, readText(declaration).replace(previous, wanted));
        assert.equal(await instruction(), previous, "the change shows only after a scan");
        if (keys) await press(browser, keys, async () => (await instruction()) === wanted, `scanned again with ${keys.join("+")}`);
        else {
          await refresh(browser).click();
          await waitFor(browser, async () => (await instruction()) === wanted, "scanned again with Refresh");
        }
        previous = wanted;
      }
      assert.equal(await camera(browser), moved, "the scans left the person's camera");
      // A click in the Detail gives it the keyboard back from the Refresh button, disabled while
      // it scanned; Esc then closes it.
      await browser.$(`${DETAIL} [role="document"]`).click();
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "docs' Detail closed");
      // A scan also redraws the graph: an Artifact that reads docs adds a node and an edge, and
      // removing it takes both away.
      await scanEnded();
      const digest = makeDirectory(nativePath(fixture.repo, "digest"));
      writeDeclaration(fixture.repo, nativePath(digest, "index.artf"), `name = "digest"

[evals.summary]
title = "Summarize the docs"
profile = ${runtimeProfile()}

[evals.summary.payload]
instruction = """
Summarize {docs}."""
`);
      await refresh(browser).click();
      await waitFor(browser, async () => {
        const { nodes, edges } = await drawn();
        return nodes["a:digest"] === "digest: waiting" && edges["a:docs>a:digest"] !== undefined;
      }, "the added digest Artifact and its relation from docs drawn");
      const waiting = await graphAppearance(browser);
      assert.notEqual(waiting.nodes["a:docs"].mark, waiting.nodes["a:digest"].mark, "would run and waiting are painted apart");
      assert.equal(await camera(browser), moved, "the new layout left the person's camera");
      await scanEnded();
      removeTree(nativePath(fixture.repo, ".artifactize", "digest.artf"));
      removeTree(digest);
      await refresh(browser).click();
      await waitFor(browser, async () => (await drawn()).nodes["a:digest"] === undefined, "the removed digest Artifact dropped");
      // Only now a real Run in this same state: the probe that found no Run would have seen one.
      const controlRun = await verifyWaiting(fixture);
      assert.deepEqual(savedRuns(fixture).map((run) => run.id), [controlRun], "the same saved-Run probe sees this fixture's real Run");
    },
  ),

  // UX journeys: J-DETAIL J-DETAIL-SUMMARY J-GRAPH-COLOURS
  scenario<{ run: string }>(
    "current-reused",
    "marks an Artifact whose every eval would be reused, its eval's tab says why with the result it would reuse, and its chip returns to its Artifact tab",
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
      await openArtifact(browser, "a:app", "Artifact app");
      assert.deepEqual(await evalRows(browser), [{ name: "alpha", status: "reused" }, { name: "beta", status: "reused" }]);
      assert.equal(await browser.$(`${DETAIL} section[aria-label="Tools of app"]`).isExisting(), false, "no Tools section for an Artifact without tools");
      await openTab(browser, "e:app/beta", { row: true });
      assert.deepEqual((await detail(browser))?.tabs.slice(1).map((tab) => [tab.name, tab.glyph]), [["alpha", "reused"], ["beta", "reused"]], "each eval's chip says it would be reused");
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("reused"), `beta's plan: ${JSON.stringify(await summary(browser))}`);
      const shown = await summary(browser);
      assert.ok(shown.blocks.includes("Result"), `the result it would reuse: ${shown.blocks}`);
      assert.equal(shown.status, `reused ${CACHED}`, "with why");
      assert.equal(shown.instruction, "Check beta.");
      // app's own chip shows its Artifact tab again, listing what each eval would do.
      await openArtifactTab(browser, "app");
      assert.deepEqual(await evalRows(browser), [{ name: "alpha", status: "reused" }, { name: "beta", status: "reused" }]);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
    },
  ),

  // UX journeys: J-MAIN-CURRENT J-DETAIL-SUMMARY J-GRAPH-COLOURS
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
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("reused"), `the sign-off's plan: ${JSON.stringify(await summary(browser))}`);
      assert.equal((await summary(browser)).status, `reused ${CACHED}`);
      const result = await block(browser, "Result");
      assert.match(result, /comment.*Checked notes 7f3a against the tag\./s, `the reused comment: ${result}`);
      assert.match(result, /approved.*true/s, `the reused approval: ${result}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // The look once every Artifact and the relation are painted reused, not a frame before
      // the relation is drawn.
      let reusedLook = await graphAppearance(browser);
      await browser.waitUntil(async () => currentHasColours(reusedLook = await graphAppearance(browser),
        { "a:docs": "reused", "a:release": "reused", "a:ship": "reused" }, { "a:release>a:ship": "reused" }),
      { timeout: 15000, timeoutMsg: "every Artifact and the relation painted reused" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(reusedLook)}`); });
      // The notes change on disk; the Current shows it only once its chip is chosen again.
      changeNotes(fixture, "# Release notes\n- A second release.\n");
      assert.deepEqual((await drawn()).nodes, allReused, "no scan without a choice, a Refresh or a Run's end");
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await drawn()).nodes["a:release"] === "release: would run (1 eval)", "release would run again");
      // The scan learns each Artifact in turn: ship's plan, once known, is to run.
      await waitFor(browser, async () => (await drawn()).nodes["a:ship"] === "ship: would run (1 eval)", "ship's fresh plan is visible");
      assert.equal((await drawn()).nodes["a:ship"], "ship: would run (1 eval)", "ship, reading the changed notes, would run again without waiting for the sign-off");
      /** Scans the app has completed, from its read-only command activity. */
      const scans = async () => (await checkpoint(browser)).activity.commands.currentScan?.completed ?? 0;
      // Its look once release, ship and their relation are painted would run.
      let look = await graphAppearance(browser);
      await browser.waitUntil(async () => currentHasColours(look = await graphAppearance(browser), { "a:release": "run", "a:ship": "run" }, { "a:release>a:ship": "run" }),
        { timeout: 15000, timeoutMsg: "would run painted in its legend colour" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(look)}`); });
      // A scan's last step comes as it ends; the header names when once it has.
      await waitFor(browser, async () => (await scannedAt(browser)) !== null, "the scan before the Run ended");
      const before = { checkpoint: await checkpoint(browser), scans: await scans(), drawn: await drawn(), look };
      assert.notEqual(before.look.nodes["a:release"].mark, reusedLook.nodes["a:release"].mark, "would run is painted unlike reused");
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
      // Completion identifies the new scan even if both scans use the same clock tick.
      await waitFor(browser, async () => (await scans()) > before.scans, "the scan triggered by the Run's end completed");
      await commandsRendered(browser, before.checkpoint);
      await waitFor(browser, async () => (await scannedAt(browser)) !== null && /^Scanned at \d/.test(await scanned(browser)),
        "the completed scan's time is rendered in the header");
      const instant = (await scannedAt(browser))!;
      const local = await browser.execute((shown: string) => new Date(shown).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" }), instant);
      assert.equal(await scanned(browser), `Scanned at ${local}`, "the header reads that scan's local time");
      const after = await graphAppearance(browser);
      for (const id of ["a:release", "a:ship"]) {
        assert.notEqual(after.nodes[id].mark, before.look.nodes[id].mark, `${id} recoloured`);
        assert.equal(after.nodes[id].mark, reusedLook.nodes[id].mark, `${id} painted as reused`);
      }
      assert.notEqual((await drawn()).edges["a:release>a:ship"], before.drawn.edges["a:release>a:ship"], "the relation relit by its source's new plan");
      // Its drawn line stays neutral throughout: only a failure colours an edge.
      const edge = "a:release>a:ship";
      assert.ok(after.edges[edge].painted, "the relation is drawn");
      assert.equal(after.edges[edge].stroke, after.palette["--edge"], "the relation stroked neutral as reused");
      assert.equal(before.look.edges[edge].stroke, after.palette["--edge"], "and as would run");
    },
  ),

  // UX journeys: J-MAIN-CURRENT J-GRAPH-COLOURS J-DETAIL J-DETAIL-SUMMARY
  scenario<{ gate: NativePath; runs: NativePath }>(
    "current-checking",
    "draws a Current's graph at once, its Artifacts being checked until their fingerprints are in, shows its running scan instead of starting over, and keeps the graph when a scan fails",
    async () => {
      const fixture = createChecking();
      return { fixture, args: ["--repo", fixture.repo], data: { gate: fixture.gate, runs: fixture.runs } };
    },
    async (browser, { fixture, data: { gate, runs } }) => {
      const checking = { ...fixture, gate, runs };
      const { drawn } = current(browser);
      const nodes = async () => JSON.stringify((await drawn()).nodes);
      const failure = () => browser.execute((main) => document.querySelector(`${main} > header .failure`)?.textContent ?? "", MAIN);
      /** The Current still shows the gated plan without starting a new fingerprint. */
      const stillChecking = async (what: string) => {
        await commandsRendered(browser);
        assert.equal(await failure(), "", `${what}: no failure shown`);
        assert.equal(await scanned(browser), "2 left", `${what}: the same scan, still checking`);
        assert.equal(fingerprintRuns(checking), 1, `${what}: the gated fingerprint did not start over`);
      };
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown");
      // other's own fingerprint is in; slow's script waits for its gate, and quick reads slow.
      const waiting = JSON.stringify({ "a:other": "other: would run (1 eval)", "a:quick": "quick: checking", "a:slow": "slow: checking" });
      await waitFor(browser, async () => (await nodes()) === waiting, `slow and quick still being checked: ${await nodes()}`);
      assert.equal(await scanned(browser), "2 left", "the header counts what is still being checked");
      const announced = await browser.execute((main) => {
        const mark = document.querySelector(`${main} > header .scan .checking`);
        return { label: mark?.getAttribute("aria-label") ?? "", icon: mark?.querySelector("svg")?.getAttribute("data-icon") ?? "" };
      }, MAIN);
      assert.deepEqual(announced, { label: "Checking, 2 Artifacts left", icon: "checking" }, "with the checking icon, announced in words");
      let look = await graphAppearance(browser);
      await browser.waitUntil(async () => currentHasColours(look = await graphAppearance(browser), { "a:other": "run", "a:quick": "checking", "a:slow": "checking" }, {}),
        { timeout: 15000, timeoutMsg: "checking painted in its own colour" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(look)}`); });
      // slow's Detail says it is still being checked, and why.
      await openArtifact(browser, "a:slow", "Artifact slow");
      assert.deepEqual(await evalRows(browser), [{ name: "check", status: "checking" }]);
      await openTab(browser, "e:slow/check", { row: true });
      await waitFor(browser, async () => (await summary(browser)).status.startsWith("checking Still being checked"),
        `slow's plan: ${JSON.stringify(await summary(browser))}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "slow's Detail closed");
      // Choosing the Current again, Refresh and the end of a Run of the branch all show the scan
      // still running: its fingerprint script runs once.
      await waitFor(browser, () => fingerprintRuns(checking) === 1, "the scan started the fingerprint script once");
      await chooseChip(browser, "repo  master", "current");
      await stillChecking("chosen again");
      writeDeclaration(fixture.repo, nativePath(fixture.repo, "added", "index.artf"), `name = "added"
[evals.check]
title = "Check the artifact added during the gated scan"
profile = ${runtimeProfile()}
payload.instruction = "Check {added}."
`);

      assert.equal((await drawn()).nodes["a:added"], undefined, "the scan still running predates the new declaration");
      const run = await verifyWaiting(fixture, ["--artifacts", "other"]);
      await waitFor(browser, async () => (await branch(browser, "repo  master"))?.chips[0]?.id === run, "the Run in progress starts the row");
      const request = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "other/signoff");
      assert.ok(request);
      signOff(fixture, request.id);
      await waitFor(browser, async () => (await branch(browser, "repo  master"))?.chips[0]?.id === "current", "the Run ended: the row starts with the Current");
      await stillChecking("the Run's end");
      assert.equal(fingerprintRuns(checking), 1, "nothing started over");
      await refresh(browser).click();
      await stillChecking("the header's Refresh button");
      // The fingerprint comes in: each Artifact is coloured in place, and the scan ends. Refresh
      // asked for the files as they are now, so one scan more follows, then the Current ends.
      writeText(gate, "go");
      const known = JSON.stringify({ "a:added": "added: would run (1 eval)", "a:other": "other: reused", "a:quick": "quick: waiting", "a:slow": "slow: would run (1 eval)" });
      await waitFor(browser, async () => (await nodes()) === known, "the follow-up scan shows the added declaration and every Artifact’s plan")
        .catch(async (error: Error) => { throw new Error(`${error.message}: ${await nodes()}; ${await scanned(browser)}; ${await failure()}`); });
      await waitFor(browser, async () => /^Scanned at \d/.test(await scanned(browser)), "the header says when it was scanned");
      await graphShown(browser, "a:added");
      assert.equal(fingerprintRuns(checking), 2, "the scan the header’s Refresh button asked for followed the one running, once");
      // Chosen again once it ended, the Current scans again, every Artifact checked again.
      removeTree(gate);
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await scanned(browser)) === "2 left", "scanned again");
      const rechecked = await drawn();
      assert.deepEqual([rechecked.nodes["a:quick"], rechecked.nodes["a:slow"]], ["quick: checking", "slow: checking"]);
      await waitFor(browser, () => fingerprintRuns(checking) === 3, "the new scan ran the fingerprint script again");
      const beforeF5 = await checkpoint(browser);
      writeDeclaration(fixture.repo, nativePath(fixture.repo, "refreshed", "index.artf"), `name = "refreshed"
[evals.check]
title = "Check the declaration introduced before F5"
profile = ${runtimeProfile()}
payload.instruction = "Check {refreshed}."
`);
      await press(browser, ["F5"], async () => (await scanned(browser)) === "2 left", "F5 leaves the second gated scan showing", { handled: true });
      assert.equal(fingerprintRuns(checking), 3, "F5 did not replace the scan already running");
      writeText(gate, "go");
      await waitFor(browser, () => fingerprintRuns(checking) === 4, "F5 also queues a follow-up scan");
      // The follow-up is queued inside the F5 request, not a second IPC call.
      // Only its graph can include the declaration added after the third scan began.
      await commandsRendered(browser, beforeF5);
      const afterF5 = { ...JSON.parse(known) as Record<string, string>, "a:refreshed": "refreshed: would run (1 eval)" };
      await waitFor(browser, async () => {
        const refreshed = (await drawn()).nodes;
        return Object.keys(refreshed).length === Object.keys(afterF5).length
          && Object.entries(afterF5).every(([id, plan]) => refreshed[id] === plan) && /^Scanned at \d/.test(await scanned(browser));
      }, "the completed F5 scan shows its new declaration, all plans and the scanned header");
      // A fingerprint that fails ends a running scan; what it showed stays.
      removeTree(gate);
      await refresh(browser).click();
      await waitFor(browser, async () => (await scanned(browser)) === "2 left", "scanning again");
      /** What a person sees of the graph: where each Artifact is and its light, each relation, all painted. */
      const visible = async () => {
        const { nodes, edges, palette } = await graphAppearance(browser);
        return {
          checking: palette["--checking"],
          nodes: Object.fromEntries(Object.entries(nodes).map(([id, node]) => [id, { at: node.at, mark: node.mark, strip: node.strip, painted: node.painted }])),
          edges: Object.fromEntries(Object.entries(edges).map(([id, edge]) => [id, { stroke: edge.stroke, painted: edge.painted }])),
        };
      };
      let shown = await visible();
      await waitFor(browser, async () => {
        shown = await visible();
        return JSON.stringify(Object.keys(shown.nodes).sort()) === JSON.stringify(["a:added", "a:other", "a:quick", "a:refreshed", "a:slow"]) &&
          shown.nodes["a:slow"].mark === shown.checking && Object.keys(shown.edges).length > 0 &&
          Object.values({ ...shown.nodes, ...shown.edges }).every((item) => item.painted);
      }, "the running scan's graph drawn")
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(shown)}`); });
      writeText(gate, "fail");
      await waitFor(browser, async () => /Fingerprint script for Artifact slow failed/.test(await failure()), "the scan's failure shown");
      assert.deepEqual(await visible(), shown, "the graph stays as it was: its Artifacts in place, its relations, all drawn");
    },
  ),

  // UX journey: J-MAIN-CURRENT
  scenario<{ first: NativePath; second: NativePath }>(
    "current-checked-progress",
    "paints independently checked Artifacts in place, showing each tally and the decreasing header count",
    async () => {
      const fixture = createGraph();
      const first = nativePath(fixture.root, "cached.gate");
      const second = nativePath(fixture.root, "fresh.gate");
      for (const [name, gate] of [["cached", first], ["fresh", second]] as const) {
        const script = commandFields("fingerprint", portablePath(gate), portablePath(nativePath(fixture.root, `${name}.reads`)));
        writeDeclaration(fixture.repo, nativePath(fixture.repo, name, "index.artf"), `name = "${name}"
fingerprint = { script = { ${script}, files = [] } }
[evals.check]
title = "Check ${name}"
profile = ${runtimeProfile()}
payload.instruction = "Check {${name}}."
`);
      }
      writeText(first, "go");
      assert.equal(artifactize(fixture, "verify", "--artifacts", "cached", "--wait"), 0, "cache one result in the real scheduler");
      removeTree(first);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { first, second } };
    },
    async (browser, { data: { first, second } }) => {
      const marks = () => browser.execute(() => Object.fromEntries(Array.from(document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node'))
        .filter((node) => ["a:cached", "a:fresh"].includes(node.dataset.id ?? ""))
        .map((node) => [node.dataset.id!, { label: node.getAttribute("aria-label"), tally: node.querySelector(".tally")?.textContent?.trim() ?? "", left: node.getBoundingClientRect().left, top: node.getBoundingClientRect().top }])));
      await graphShown(browser, "a:cached");
      await waitFor(browser, async () => (await scanned(browser)) === "2 left" && Object.values(await marks()).every((node) => node.label?.endsWith("checking") && node.tally === ""), "both independent nodes show checking without any tally");
      await camera(browser);
      const before = await marks();
      assert.equal(Object.keys(before).length, 2);
      const inPlace = (now: Awaited<ReturnType<typeof marks>>, id: string) => Math.abs(now[id].left - before[id].left) <= 1 && Math.abs(now[id].top - before[id].top) <= 1;
      writeText(first, "go");
      await waitFor(browser, async () => {
        const now = await marks();
        return (await scanned(browser)) === "1 left" && now["a:cached"]?.label === "cached: reused" && now["a:cached"].tally === "1/1" &&
          now["a:fresh"]?.label === "fresh: checking" && now["a:fresh"].tally === "" &&
          currentHasColours(await graphAppearance(browser), { "a:cached": "reused", "a:fresh": "checking" }, {}) && inPlace(now, "a:cached");
      }, "only the released node changes to reused in place and the header falls to one left");
      writeText(second, "go");
      await waitFor(browser, async () => {
        const now = await marks();
        return /^Scanned at \d/.test(await scanned(browser)) && now["a:fresh"]?.label === "fresh: would run (1 eval)" && now["a:fresh"].tally === "0/1" &&
          currentHasColours(await graphAppearance(browser), { "a:cached": "reused", "a:fresh": "run" }, {}) && inPlace(now, "a:fresh");
      }, "the last node gains its would-run tally in place and the header changes to the scan time");
    },
  ),

  // UX journeys: J-MAIN-CURRENT J-GRAPH-COLOURS
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
      // Drawn as a Run's Artifacts are: each node shows its name and how many of its evals would
      // be reused of how many, its plan by its mark and colour.
      const texts = await browser.execute(() => Object.fromEntries([...document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node')]
        .filter((node) => !node.closest("aside")).map((node) => [node.dataset.id ?? "", `${node.querySelector(".name")?.textContent?.trim()} ${node.querySelector(".tally")?.textContent?.trim()}`])));
      assert.deepEqual(texts, { "a:app": "app 1/3", "a:down": "down 0/1", "a:red": "red 1/1" });
      const look = await graphAppearance(browser);
      assert.notEqual(look.nodes["a:down"].mark, look.nodes["a:app"].mark, "blocked is painted unlike would run");
      assert.notEqual(look.nodes["a:down"].mark, look.nodes["a:red"].mark, "blocked is painted unlike reused");
      assert.ok(currentHasColours(look, { "a:app": "run", "a:down": "blocked", "a:red": "reused" }, {}),
        `each Artifact in its own colour: ${JSON.stringify(look)}`);
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
      await waitFor(browser, async () => (await summary(browser)).status === `reused ${CACHED}`, `alpha's plan: ${JSON.stringify(await summary(browser))}`);
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "app's Detail closed");
      // down cannot be evaluated while red is RED, and says so.
      await openArtifact(browser, "a:down", "Artifact down");
      assert.deepEqual(await evalRows(browser), [{ name: "check", status: "blocked" }]);
      await openTab(browser, "e:down/check", { row: true });
      await waitFor(browser, async () => /^blocked \S.*red\/check/.test((await summary(browser)).status),
        `down's plan is blocked and names the RED eval that blocks it: ${JSON.stringify(await summary(browser))}`);
    },
  ),
];

