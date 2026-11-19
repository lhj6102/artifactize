// Moving through the window in the real app: the keys of what has focus, a refresh, the
// pointer that only highlights, the Pane's keys through branches, timelines and waiting reviews
// into the Main area and back, a Detail stepped back out of, unreadable Runs beside a readable
// one; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { requestState, reviewing, showDebug } from "../human-review.ts";
import { addRepository, addWaiting, create, createGraph, createHistory, createNavigationFixture, createUnreadableRuns, orderRuns, requests, signoff, verifyCommented, verifyWaiting } from "../fixture.ts";
import { git } from "../environment.ts";
import { afterRender, fill, press } from "../keys.ts";
import { checkpoint, commandsRendered, refreshed } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { sessionEndedByApp } from "../observation.ts";
import { primaryModifier, requestWindowClose, shortcutLabel } from "../platform.ts";
import { nativePath, readText, writeText } from "../portable.ts";
import { pointerGapClick, visibleCenter } from "../pointer.ts";
import { aside, camera, detail, fact, graphShown, nodeAt, openArtifact, openTab, summary } from "../graph-view.ts";
import { branch, branches, chooseChip, entriesFocused, expand, listed, MAIN, mainFocused, mainTitle, openWaiting, scanned, SEARCH, showRun, waiting } from "../pane.ts";
import { choosePaletteCommand } from "../palette-commands.ts";

type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture"> & { review: string };

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The marks the open Help dialog pairs with their words: each mark's icon and its word. */
function helpMarks(browser: WebdriverIO.Browser) {
  return browser.execute(() => [...document.querySelectorAll('[role="dialog"].help [aria-label="Marks"] li')]
    .map((item) => [item.querySelector("svg")?.getAttribute("data-icon") ?? "", item.textContent?.trim() ?? ""]));
}

/** A Run's graph's marks, as Help pairs them: as a Run goes, then what went wrong. */
const RUN_MARKS = [["green", "GREEN"], ["running", "in progress"], ["waiting", "waiting"], ["red", "RED"], ["error", "ERROR"], ["blocked", "blocked by upstream"]];
/** A Current's marks: what verify would do with each Artifact now. */
const CURRENT_MARKS = [["reused", "reused"], ["run", "would run"], ["waiting", "waiting"], ["blocked", "blocked by upstream"], ["checking", "checking"]];

/** The keys the open Help dialog lists, each with what it does. */
function shortcuts(browser: WebdriverIO.Browser) {
  return browser.execute(() => [...document.querySelectorAll('[role="dialog"].help dt')].map((key) => ({
    keys: key.textContent?.trim() ?? "",
    action: key.nextElementSibling?.textContent?.trim() ?? "",
  })));
}

export const scenarios: Scenario[] = [
  // UX journey: J-HELP
  scenario<Record<string, never>>(
    "navigation-help",
    "lists the keys of the Pane, the Main area or Settings, whichever has focus, with ? and closes them with Esc",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      // Help, not Settings, which is also a dialog.
      const helpOpen = () => browser.$('[role="dialog"].help').isExisting();
      /** Each pair's keys as a pattern, or as the exact label the platform shows. */
      const listed = async (pairs: [RegExp | string, RegExp][], absent: RegExp[], where: string) => {
        await press(browser, ["?"], helpOpen, `the keys of ${where}`);
        assert.match(await browser.$('[role="dialog"].help').getText(), /Keyboard shortcuts/);
        const shown = await shortcuts(browser);
        for (const [keys, action] of pairs) {
          const matches = (label: string) => typeof keys === "string" ? label === keys : keys.test(label);
          assert.ok(shown.some((entry) => matches(entry.keys) && action.test(entry.action)), `${where}'s Help pairs ${keys} with ${action}: ${JSON.stringify(shown)}`);
        }
        for (const keys of absent) assert.ok(!shown.some((entry) => keys.test(entry.keys)), `${where}'s Help leaves out ${keys}`);
        await press(browser, ["Escape"], async () => !(await helpOpen()), `${where}'s keys closed`);
      };
      await graphShown(browser, "a:release");
      const helpFromPalette = async () => {
        await browser.$("button*=Commands").click();
        await choosePaletteCommand(browser, "Keyboard shortcuts");
        await browser.$('.palette [role="option"][aria-selected="true"]').click();
        await waitFor(browser, helpOpen, "the palette opens Help");
      };
      await helpFromPalette();
      await browser.$('[role="dialog"].help').$("button=Close").click();
      await waitFor(browser, async () => !(await helpOpen()), "Help closes by its Close button");
      await helpFromPalette();
      const outside = await browser.execute(() => {
        const box = document.querySelector('[role="dialog"].help')!.getBoundingClientRect();
        return [{ x: 8, y: 8 }, { x: 8, y: innerHeight - 8 }, { x: innerWidth - 8, y: innerHeight - 8 }]
          .find(({ x, y }) => x < box.left || x >= box.right || y < box.top || y >= box.bottom) ?? null;
      });
      assert.ok(outside, "Help leaves some viewport outside its panel");
      await pointerGapClick(browser, outside);
      await waitFor(browser, async () => !(await helpOpen()), "Help closes by an outside click");
      await browser.$('[aria-label="Pane entries"]').click();
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      /** The marks the open Help pairs with their words; empty without any. */
      const marks = async () => {
        await press(browser, ["?"], helpOpen, "Help open for its marks");
        const shown = await helpMarks(browser);
        await press(browser, ["Escape"], async () => !(await helpOpen()), "Help closed");
        return shown;
      };
      await listed([
        [/^↑ \/ ↓ · j \/ k$/, /^Select a branch, a timeline entry or a waiting review$/],
        [/^← \/ →$/, /^Fold or expand the branch/],
        [/^Esc$/, /sort:recent/],
        [/\bF5\b/, /^Refresh$/],
        [shortcutLabel("Ctrl+O"), /^Open a request by ID$/],
        [shortcutLabel("Ctrl+B"), /^Show or hide the Pane$/],
        [shortcutLabel("Ctrl+K"), /^Command palette$/],
      ], [/^m$/, /^c$/], "the Pane");
      assert.deepEqual(await marks(), [], "the Pane's Help has no marks");
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await listed([
        [/^Tab$/, /^Next Artifact$/],
        [/^Enter$/, /^Open its Detail$/],
        [shortcutLabel("Ctrl+F"), /^Find an Artifact or an eval$/],
        [/^m$/, /^Minimize or expand a whole$/],
        [/\bF5\b/, /^Refresh$/],
      ], [/^↑ \/ ↓ · j \/ k$/, /^c$/], "the Main area");
      // On a Run's graph, Help pairs each of its marks with its word.
      assert.deepEqual(await marks(), RUN_MARKS, "the Run's marks, each with its word");
      // The palette's Help, opened over the Run's graph, shows the same legend.
      await helpFromPalette();
      assert.deepEqual(await helpMarks(browser), RUN_MARKS, "Help from the palette pairs the Run's marks with their words");
      await browser.$('[role="dialog"].help').$("button=Close").click();
      await waitFor(browser, async () => !(await helpOpen()), "Help closed by its Close button");
      await openArtifact(browser, "a:release", "Artifact release");
      const debug = aside(browser).$('section[aria-label="Debug"] > button');
      await debug.waitForDisplayed({ timeout: 15000 });
      await debug.click();
      await waitFor(browser, () => debug.isFocused(), "the Detail's Debug header has focus");
      await listed([[/^t$/, /^Show or hide Debug$/], [/^Esc$/, /^Close the Detail/]], [/^c$/], "the graph Detail");
      const beneath = await mainTitle(browser);
      await press(browser, [primaryModifier, ","], () => browser.$("#settings-title").isExisting(), "Settings open");
      assert.equal(await mainTitle(browser), beneath, "Settings opens over the Main area's view");
      await listed([[/^Esc$/, /^Close Settings$/], [/^← \/ →$/, /^Previous or next tab$/]], [/^m$/, /^c$/], "Settings");
    },
  ),

  // UX journey: J-LIVE
  scenario<{ run: string }>(
    "navigation-live-run",
    "shows a Run another process starts on the shown branch without a reload",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: { run: requests(fixture)[0].runId } };
    },
    async (browser, { fixture, data: { run } }) => {
      await graphShown(browser, "a:release");
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id), [run]);
      const id = await verifyWaiting(fixture);
      // Explicitly the newer, whatever times the two verifies were saved at.
      orderRuns(fixture, [run, id]);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([id, run]),
        "the new Run arrives in the timeline without F5");
      assert.equal(await mainTitle(browser), run, "the shown Run stays shown");
    },
  ),

  // UX journeys: J-REFRESH J-MAIN-CURRENT
  scenario<{ worktree: string }>(
    "navigation-refresh",
    "reloads the state, scans the branches and rescans the shown Current on the same view",
    async () => {
      const fixture = await create();
      git(fixture.repo, "-C", fixture.repo, "add", ".");
      git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Refresh fixture inputs");
      return { fixture, args: ["--repo", fixture.repo], data: { worktree: nativePath(fixture.root, "after-refresh") } };
    },
    async (browser, { fixture, data }) => {
      await graphShown(browser, "a:release");
      const run = requests(fixture)[0].runId;
      const comment = "A Run started outside the window";
      const timeline = (id: string) => browser.$(`[data-chip="${id}"] .sub`);
      const timelineShows = async (id: string, text: string) => await timeline(id).isDisplayed() && await timeline(id).getText() === text;
      await waitFor(browser, () => timelineShows(run, run), "the Run has no comment");
      assert.equal(await mainTitle(browser), run, "the Run is shown before refreshing");
      assert.equal(await branch(browser, "repo  after-refresh"), undefined, "the new worktree is not listed yet");
      const before = await checkpoint(browser);
      // Another process starts a Run with a comment.
      const started = verifyCommented(fixture, comment);
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "after-refresh", data.worktree);
      // Background reconciliation cannot stand in for the actual reload: bind the two
      // external changes to this command's own reply, then check they are drawn together.
      await browser.$("header.bar").$("button*=Commands").click();
      await choosePaletteCommand(browser, "Refresh");
      await browser.$('.palette [role="option"][aria-selected="true"]').click();
      // The reload's completion is the barrier; what it read is checked in the window.
      await refreshed(browser, before);
      await waitFor(browser, async () => await timelineShows(started, comment) && !!(await branch(browser, "repo  after-refresh")),
        "the new Run and the new worktree are rendered together");
      assert.equal(await mainTitle(browser), run, "on the same view");
      assert.equal(await browser.$(`${MAIN} > header [role="status"]`).isExisting(), false, "the refresh read without an error");
      // The new branch's Current: only a refresh or its Refresh button scans it again.
      await chooseChip(browser, "repo  after-refresh", "current");
      await waitFor(browser, async () => /^Scanned at /.test(await scanned(browser)), "its Current scanned");
      // docs' eval open in the Detail shows its instruction as the last scan read it.
      await openArtifact(browser, "a:docs", "Artifact docs");
      await openTab(browser, "e:docs/lint");
      const instruction = async () => (await summary(browser)).instruction;
      await waitFor(browser, async () => (await instruction()) === "Build the docs.", "docs' eval open");
      const declaration = nativePath(data.worktree, ".artifactize", "docs.artf");
      const previous = "Build the docs.";
      const wanted = "Build the docs after rescanning.";
      writeText(declaration, readText(declaration).replace(previous, wanted));
      assert.equal(await instruction(), previous, "the changed declaration is not shown before the refresh");
      await browser.$(`${MAIN} > header`).$("button=Refresh").click();
      await waitFor(browser, async () => (await instruction()) === wanted && /^Scanned at /.test(await scanned(browser)),
        "the Refresh button scanned the Current and its Detail shows the changed declaration");
      assert.equal(await mainTitle(browser), "Current · repo  after-refresh", "on the same view");
      // The palette's Refresh rescans the shown Current too.
      const again = "Build the docs after the palette's Refresh.";
      writeText(declaration, readText(declaration).replace(wanted, again));
      assert.equal(await instruction(), wanted, "the second change is not shown before the refresh");
      await browser.$("header.bar").$("button*=Commands").click();
      await choosePaletteCommand(browser, "Refresh");
      await browser.$('.palette [role="option"][aria-selected="true"]').click();
      await waitFor(browser, async () => (await instruction()) === again && /^Scanned at /.test(await scanned(browser)),
        "the palette's Refresh scanned the Current and its Detail shows the changed declaration");
      assert.equal(await mainTitle(browser), "Current · repo  after-refresh", "still on the same view");
    },
  ),

  scenario<Record<string, never>>(
    "navigation-technical-detail",
    "shows the Debug facts of an eval's Summary with t and closes the Detail back to the graph",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:ship");
      await openArtifact(browser, "a:ship", "Artifact ship");
      await openTab(browser, "e:ship/check");
      const request = requests(fixture).find((saved) => saved.evalId === "ship/check");
      assert.ok(request, "the fixture independently identifies ship/check");
      await browser.waitUntil(async () => (await summary(browser)).instruction.startsWith("Ship"), { timeout: 15000, timeoutMsg: "ship/check's Summary" });
      assert.equal((await summary(browser)).debug, false, "Debug starts folded");
      assert.equal(await fact(browser, "Request"), "");
      assert.equal(await fact(browser, "Waits for"), "", "the waiting eval's input facts start hidden");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await summary(browser)).debug === true && (await fact(browser, "Request")) === request.id
        && (await fact(browser, "Waits for")).includes("release"), "the request and its waiting input inside Debug");
      assert.match(await fact(browser, "Waits for"), /signoff/);
      await press(browser, ["t"], async () => (await summary(browser)).debug === false && (await fact(browser, "Request")) === ""
        && (await fact(browser, "Waits for")) === "", "the Debug facts and waits hidden again");
      assert.ok((await summary(browser)).instruction.startsWith("Ship"), "folding Debug keeps the Summary");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // The graph as it was, every Artifact still drawn and reachable.
      for (const id of ["a:release", "a:ship", "a:docs"]) await graphShown(browser, id);
    },
  ),

  // The pointer rule of the UX journeys: hovering opens nothing; information opens on a click.
  scenario<Record<string, never>>(
    "navigation-hover-opens-nothing",
    "opens nothing for the Artifact or the timeline entry under the pointer, and opens the Artifact's Detail on a click",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:ship");
      // Where the pointer's moves landed, logged before any handler without changing them.
      await browser.execute(() => {
        const log: string[] = [];
        (window as unknown as { overs: string[] }).overs = log;
        document.addEventListener("mousemove", (event) => {
          const target = (event.target as Element | null)?.closest?.(".svelte-flow__node, button[data-chip]");
          if (target) log.push(target.getAttribute("data-id") ?? target.getAttribute("data-chip") ?? "");
        }, true);
      });
      const look = () => browser.execute((selector) => ({
        over: (window as unknown as { overs: string[] }).overs.slice(),
        opened: !!document.querySelector('aside[aria-label="Detail"], [aria-label="Summary"], [aria-label^="Evals of "], [role="tooltip"], [role="menu"]'),
        tooltip: !!document.querySelector(selector)?.querySelector("[title]") || !!document.querySelector(selector)?.closest("[title]")
          || !!document.querySelector("button[data-chip][title], button[data-chip] [title]"),
      }), nodeAt("a:ship"));
      // A hover that opened something only after a delay must still be seen: the pointer stays
      // put while frames render, past the engine's tooltip delay, and nothing may open.
      const HOVER_WINDOW_MS = 1500;
      const nothingOpensWhileHovering = async () => {
        const until = performance.now() + HOVER_WINDOW_MS;
        while (performance.now() < until) {
          if ((await look()).opened) return false;
          await afterRender(browser);
        }
        return !(await look()).opened;
      };
      const chip = await browser.$("button[data-chip]");
      const chipId = await chip.getAttribute("data-chip");
      assert.ok(chipId, "the timeline entry has an identity");
      await browser.action("pointer").move(await visibleCenter(browser, await chip.getElement())).perform();
      await browser.waitUntil(async () => (await look()).over.includes(chipId), { timeout: 15000, timeoutMsg: "the pointer moved over the named timeline entry" });
      assert.ok(await nothingOpensWhileHovering(), "hovering the timeline entry opens nothing, then or later");
      const overChip = await look();
      assert.equal(overChip.tooltip, false, "the timeline entry has no native tooltip");
      const point = await visibleCenter(browser, await browser.$(nodeAt("a:ship")).getElement());
      await browser.action("pointer").move(point).perform();
      await browser.waitUntil(async () => (await look()).over.includes("a:ship"), { timeout: 15000, timeoutMsg: "the pointer moved over ship" });
      assert.ok(await nothingOpensWhileHovering(), "hovering opens no Detail, summary, eval list, tooltip or menu, then or later");
      const hovered = await look();
      assert.equal(hovered.tooltip, false, "neither the Artifact nor a timeline entry carries a tooltip");
      // The same probe sees what a click opens.
      await browser.$(nodeAt("a:ship")).click();
      await browser.waitUntil(async () => (await look()).opened && (await detail(browser))?.title === "Artifact ship",
        { timeout: 15000, timeoutMsg: `a click opened ship's Detail: ${JSON.stringify(await detail(browser))}` });
    },
  ),

  // UX journey: J-WAITING (also exercises optional keyboard navigation)
  scenario<NavigationData>(
    "navigation-pane-keys",
    "moves through the branches and then the waiting reviews by keys, expands a branch and moves through its timeline, folds it, enters the Main area and comes back",
    async () => {
      const { fixture, ...data } = await createNavigationFixture();
      // The gates Run is dated after the fixture's other Runs, so it is the newest.
      const [review] = await addWaiting(fixture, "gates", 1, "2000-01-04T00:00:00.000000000Z");
      return { fixture, data: { ...data, review } };
    },
    async (browser, { data: example }) => {
      const [older, newer] = example.runs;
      // The most recent Run first: the gates (the newest date), other (third), then repo.
      await waitFor(browser, async () => (await branches(browser)).length === 3, "the three live branches");
      assert.deepEqual((await branches(browser)).map((entry) => entry.name), ["gates  master", "other  master", "repo  master"]);
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      const selected = async () => (await branches(browser)).find((entry) => entry.selected)?.name
        ?? (await waiting(browser)).rows.find((entry) => entry.selected)?.id ?? null;
      const filled = async (name: string) => (await branch(browser, name))?.chips.find((chip) => chip.filled)?.id ?? null;
      const expanded = async (name: string) => (await branch(browser, name))?.expanded === true;
      // ↓ and j select the next branch, which shows its newest entry; k and ↑ go back.
      await press(browser, ["ArrowDown"], async () => (await selected()) === "gates  master" && /^run-/.test(await mainTitle(browser)), "the first branch's Run in progress shown");
      await press(browser, ["j"], async () => (await selected()) === "other  master" && (await mainTitle(browser)) === "Current · other  master", "the next branch's Current shown");
      await press(browser, ["ArrowDown"], async () => (await selected()) === "repo  master" && (await mainTitle(browser)) === "Current · repo  master", "repo's Current shown");
      // → expands it; → again enters its timeline on its Current; ↓ and ↑ move through its Runs,
      // each shown and marked.
      await press(browser, ["ArrowRight"], () => expanded("repo  master"), "repo expanded");
      assert.equal(await selected(), "repo  master", "expanding leaves the branch selected");
      await press(browser, ["ArrowRight"], () => browser.execute(() => !!document.querySelector(
        '[aria-label="Branches"] > [role="treeitem"][aria-selected="false"] [role="treeitem"][aria-selected="true"] button[data-chip="current"]',
      )), "the keyboard selection enters repo's Current");
      assert.equal(await mainTitle(browser), "Current · repo  master");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === newer.id && (await filled("repo  master")) === newer.id, "the newer Run shown");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === older.id && (await filled("repo  master")) === older.id, "the older Run shown");
      await press(browser, ["ArrowUp"], async () => (await filled("repo  master")) === newer.id, "the newer Run again");
      await press(browser, ["ArrowUp"], async () => (await filled("repo  master")) === "current", "its Current again");
      await press(browser, ["ArrowDown"], async () => (await filled("repo  master")) === newer.id, "back on the newer Run");
      // ← steps back out to the branch, keeping what is shown; ← again folds it.
      await press(browser, ["ArrowLeft"], async () => (await browser.execute(() =>
        document.querySelector('[aria-label="Branches"] > [aria-selected="true"]')?.getAttribute("aria-expanded"))) === "true", "the branch itself selected");
      await press(browser, ["ArrowLeft"], async () => !(await expanded("repo  master")), "repo folded");
      assert.equal(await mainTitle(browser), newer.id, "stepping out and folding keep what is shown");
      // Enter goes into the Main area, on the shown Run: its own historical criterion.
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await graphShown(browser, "a:subject");
      const opened = await openArtifact(browser, "a:subject", "Artifact subject");
      assert.deepEqual(opened.evals.map((row) => row.name), [newer.eval], "only the shown Run's criterion");
      await openTab(browser, `e:subject/${newer.eval}`);
      const human = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"][tabindex="0"]');
      await waitFor(browser, () => browser.execute((name) => !!document.querySelector('aside[aria-label="Detail"] [aria-label="Human review"]')
        && (document.querySelector('aside[aria-label="Detail"] h2')?.textContent?.includes(`subject/${name}`) ?? false),
        newer.eval), "the eval's tab holds its Human review");
      await human().click();
      // Esc leaves the review's Detail, back on the graph; Esc again returns to the Pane.
      await press(browser, ["Escape"], async () => (await detail(browser)) === null && await mainFocused(browser), "the Detail closed, the Main area focused");
      await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane");
      assert.equal(await selected(), "repo  master", "the same entry stays selected");
      // Past the last branch the waiting reviews: one is only selected, Enter opens it.
      await press(browser, ["j"], async () => (await selected()) === example.review, "the waiting review selected");
      assert.equal(await mainTitle(browser), newer.id, "selecting a waiting review opens nothing");
      await press(browser, ["Enter"], async () => (await mainTitle(browser)) === "Review", "Enter opened the review");
      await showDebug(browser);
      await waitFor(browser, async () => (await requestState(browser)).startsWith(`Request ${example.review} · DEFERRED`),
        "the selected review opened");
      // Esc in a review opened from the Pane returns to the Pane, the review still selected.
      await browser.$('[aria-label="Human review"]').click();
      await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane from the review");
      assert.equal(await selected(), example.review);
      await press(browser, ["k"], async () => (await selected()) === "repo  master", "k back to the last branch");
    },
  ),

  // Regression: optional keyboard navigation within a timeline.
  scenario<{ runs: string[] }>(
    "navigation-timeline-keys",
    "moves by ↓ and ↑ through a timeline longer than the Pane, the list scrolling so that each chosen entry shows, and asks for older Runs past the oldest",
    async () => {
      const { fixture, runs } = createHistory(30);
      return { fixture, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      const LIST = '[role="tree"][aria-label="Branches"]';
      /** The list's scroll offset, whether it overflows, and the marked entry with whether all of it shows within the list. */
      const shown = () => browser.execute((selector) => {
        const list = document.querySelector<HTMLElement>(selector)!;
        const box = list.getBoundingClientRect();
        const entry = list.querySelector<HTMLElement>('button[data-chip][aria-pressed="true"]');
        const at = entry?.getBoundingClientRect();
        return {
          scroll: Math.round(list.scrollTop),
          overflows: list.scrollHeight > list.clientHeight,
          filled: entry?.dataset.chip ?? null,
          visible: !!at && at.top >= box.top - 1 && at.bottom <= box.bottom + 1,
        };
      }, LIST);
      await listed(browser, "repo  master");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === "Current · repo  master", "the branch's Current shown");
      if (!(await branch(browser, "repo  master"))?.expanded) await press(browser, ["ArrowRight"], async () => (await branch(browser, "repo  master"))?.expanded === true, "expanded");
      await press(browser, ["ArrowRight"], async () => (await shown()).filled === "current", "on its Current");
      const held = (await branch(browser, "repo  master"))!.chips.length - 1;
      assert.ok(held < runs.length, `older Runs are left for later: ${held} of ${runs.length}`);
      const start = (await shown()).scroll;
      // On through every Run, newest to oldest, downwards: each chosen entry shows within the list.
      // On the oldest held, ↓ first lists older Runs below it, the selection staying; ↓ again
      // moves on into them.
      for (const id of [...runs].reverse()) {
        const held = (await branch(browser, "repo  master"))!.chips;
        const at = (await shown()).filled;
        if (held[held.length - 1].id === at) {
          const title = await mainTitle(browser);
          await press(browser, ["ArrowDown"], async () => (await branch(browser, "repo  master"))!.chips.length > held.length, "older Runs listed below the oldest held");
          assert.equal((await shown()).filled, at, "the selection stays while older Runs are listed");
          assert.equal(await mainTitle(browser), title, "the Main area still shows the selected entry");
        }
        await press(browser, ["ArrowDown"], async () => (await shown()).filled === id && (await mainTitle(browser)) === id, `${id} chosen`);
        await waitFor(browser, async () => (await shown()).visible, `${id} shows within the list: ${JSON.stringify(await shown())}`);
      }
      assert.equal((await branch(browser, "repo  master"))!.chips.length, runs.length + 1, "the older Runs arrived on the way");
      assert.ok((await shown()).overflows && (await shown()).scroll > start, "the list scrolled toward the oldest to show them");
      // Back up to the Current, the list scrolling back to show each.
      for (const id of [...runs.slice(1), "current"]) {
        await press(browser, ["ArrowUp"], async () => (await shown()).filled === id, `${id} chosen again`);
        await waitFor(browser, async () => (await shown()).visible, `${id} shows within the list: ${JSON.stringify(await shown())}`);
      }
      assert.equal(await mainTitle(browser), "Current · repo  master");
    },
  ),

  // UX journey: J-MAIN-RUN
  scenario<{ readable: string; unreadable: string[] }>(
    "navigation-unreadable-runs",
    "keeps a shown Run's graph within the Main area while its notice names the Runs that cannot be read, in full on a click",
    async () => {
      const { fixture, readable, unreadable, restore } = createUnreadableRuns();
      return { fixture, args: ["--repo", fixture.repo], data: { readable, unreadable }, restore: async () => restore() };
    },
    async (browser, { data: { readable, unreadable } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", readable);
      const pane = () => browser.execute((main) => {
        const section = document.querySelector<HTMLElement>(main);
        const heading = section?.querySelector<HTMLElement>("h2");
        const notice = section?.querySelector<HTMLElement>(':scope > header [role="status"]');
        const toggle = notice?.querySelector<HTMLElement>("button");
        const node = section?.querySelector<HTMLElement>('.svelte-flow__node[data-id="a:n000"]');
        const divider = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
        if (!section || !heading || !notice || !toggle || !node || !divider) return null;
        const box = section.getBoundingClientRect();
        const within = (element: HTMLElement) => {
          const inner = element.getBoundingClientRect();
          return inner.width > 0 && inner.left >= box.left && inner.right <= box.right;
        };
        const line = divider.getBoundingClientRect();
        const range = document.createRange();
        range.selectNodeContents(toggle);
        const fragments = [...range.getClientRects()].filter((part) => part.width > 0 && part.height > 0);
        const clip = notice.getBoundingClientRect();
        return {
          fragments: fragments.length,
          visible: fragments.flatMap((part, index) => part.left >= clip.left && part.right <= clip.right + 1
            && part.top >= clip.top && part.bottom <= clip.bottom && part.bottom <= innerHeight
            && toggle.contains(document.elementFromPoint(part.x + part.width / 2, part.y + part.height / 2)) ? [index] : []),
          scroll: notice.scrollTop,
          scrollEnd: notice.scrollHeight - notice.clientHeight,
          scrollPoint: { x: Math.round(clip.x + clip.width / 2), y: Math.round(clip.y + clip.height / 2) },
          inWindow: box.left >= 0 && box.right <= window.innerWidth,
          bodyWithin: [...section.children].every((child) => within(child as HTMLElement)),
          graph: (() => { const at = section.querySelector(".graph")!.getBoundingClientRect(); return [at.left, at.top, at.width, at.height].map(Math.round); })(),
          nodeWithin: within(node),
          noticeWithin: within(notice),
          titleWhole: within(heading) && heading.scrollWidth <= heading.clientWidth,
          // The divider between the Pane and the Main area stays on screen, left of it.
          divider: line.width > 0 && line.left > 0 && line.right <= box.left + 1,
          text: toggle.textContent ?? "",
          cut: toggle.scrollWidth > toggle.clientWidth,
          open: toggle.getAttribute("aria-expanded") === "true",
          tall: toggle.getBoundingClientRect().height > heading.getBoundingClientRect().height * 2,
        };
      }, MAIN);
      let last: Awaited<ReturnType<typeof pane>> = null;
      await browser.waitUntil(async () => {
        last = await pane();
        return !!last && last.inWindow && last.bodyWithin && last.nodeWithin && last.noticeWithin && last.titleWhole && last.divider;
      }, { timeout: 15000, timeoutMsg: "the Main area keeps its title, notice and graph within its width" })
        .catch((error: unknown) => { throw new Error(`${String(error)}; the Main area last showed ${JSON.stringify(last)}`); });
      await graphShown(browser, "a:n000");
      const folded = (await pane())!;
      assert.ok(folded.cut && !folded.open, "the notice is one line, cut to the header's room");
      let expanded: Awaited<ReturnType<typeof pane>> = null;
      // A click shows the notice in full; the graph stays where it was. Another folds it.
      const toggle = () => browser.$(`${MAIN} > header [role="status"] button`);
      await toggle().click();
      await browser.waitUntil(async () => {
        expanded = await pane();
        return !!expanded && expanded.open && !expanded.cut && expanded.tall && expanded.bodyWithin && expanded.inWindow
          && expanded.visible.length > 0 && unreadable.every((id) => expanded!.text.includes(`Unreadable run ${id}: unknown variant \`dependency\``));
      }, { timeout: 15000, timeoutMsg: "the unreadable Runs' messages are laid out in the expanded notice" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(expanded)}`); });
      let at = expanded!;
      assert.doesNotMatch(at.text, new RegExp(readable), "the readable Run is not named unreadable");
      const seen = new Set(at.visible);
      // The header stays in place. Read its wrapped lines through its own scroll window.
      while (at.scroll < at.scrollEnd && seen.size < at.fragments) {
        const before = at.scroll;
        await browser.action("wheel").scroll({ ...at.scrollPoint, deltaX: 0, deltaY: 8 }).perform();
        await browser.waitUntil(async () => {
          const shown = await pane();
          if (shown && shown.scroll > before) { at = shown; return true; }
          return false;
        }, { timeout: 15000, timeoutMsg: "the expanded notice scrolls within the header" });
        for (const index of at.visible) seen.add(index);
      }
      assert.equal(seen.size, at.fragments, "every line of the full messages was visible in the scrolled notice");
      assert.deepEqual((await pane())?.graph, folded.graph, "opening the full notice does not move or resize the graph");
      await graphShown(browser, "a:n000");
      await toggle().click();
      await browser.waitUntil(async () => {
        const shown = await pane();
        return !!shown && !shown.open && shown.cut;
      }, { timeout: 15000, timeoutMsg: "the notice folded back to one line" });
      assert.deepEqual((await pane())?.graph, folded.graph, "collapsing the notice keeps the same graph geometry");
      await graphShown(browser, "a:n000");
    },
  ),

  // UX journey: J-QUIT
  scenario<Record<string, never>>(
    "navigation-quit-clean",
    "closes the window at once with q",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      // Observe the real window's exit before retiring the ended WebDriver client session.
      await sessionEndedByApp(browser, () => browser.keys(["q"]));
    },
  ),

  // UX journey: J-QUIT
  scenario<Record<string, never>>(
    "navigation-quit-palette",
    "quits at once through the palette's Quit",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await browser.$("header.bar").$("button*=Commands").click();
      await choosePaletteCommand(browser, "Quit");
      // Observe the real window's exit before retiring the ended WebDriver client session.
      await sessionEndedByApp(browser, () => browser.$('.palette [role="option"][aria-selected="true"]').click());
    },
  ),

  // UX journey: J-QUIT
  scenario<Record<string, never>>(
    "navigation-close-clean",
    "closes at once, asking nothing",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await sessionEndedByApp(browser, () => requestWindowClose(browser));
    },
  ),
  // UX journey: J-PANE-TOGGLE
  scenario<Record<string, never>>(
    "navigation-pane-toggle",
    "hides the Pane by Ctrl+B, its switch or the palette, the Main area taking the window's width with its view as it was, and shows it again as it was: its width, search, selection and expanded branches",
    async () => {
      const fixture = await create();
      await addRepository(fixture, "other");
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const toggle = () => browser.$('button[aria-label="Pane (Ctrl+B)"]');
      /** The Pane's width (null while hidden), whether the divider is there, and the Main area's and the window's widths. */
      const layout = () => browser.execute(() => {
        const pane = document.querySelector<HTMLElement>('section[aria-label="Pane"]');
        const stage = document.querySelector<HTMLElement>(".monitor .stage")!.getBoundingClientRect();
        return {
          pane: pane ? Math.round(pane.getBoundingClientRect().width) : null,
          divider: !!document.querySelector(".monitor [data-pane-resizer]"),
          main: Math.round(stage.width),
          window: document.documentElement.clientWidth,
        };
      });
      /** What the Pane holds: its search, and each branch's selection and expansion. */
      const held = async () => ({
        query: await browser.$(SEARCH).getValue(),
        branches: (await branches(browser)).map((entry) => [entry.name, entry.selected, entry.expanded]),
      });
      await graphShown(browser, "a:release");
      await listed(browser, "other  master");
      // A Pane as the person left it: dragged wider, a search of their own, both branches expanded.
      const divider = () => browser.$(".monitor [data-pane-resizer]");
      const narrow = (await layout()).pane!;
      await browser.action("pointer", { id: "divider" }).move({ origin: await divider(), x: 2, y: 0 }).down().perform(true);
      try {
        await afterRender(browser);
        const pointer = browser.action("pointer", { id: "divider" });
        for (let step = 0; step < 4; step += 1) pointer.move({ origin: "pointer", x: 20, y: 0 });
        await pointer.perform(true);
        await browser.waitUntil(async () => ((await layout()).pane ?? 0) > narrow + 40, { timeout: 15000, timeoutMsg: "the dragged Pane" });
        await browser.action("pointer", { id: "divider" }).up().perform();
      } finally { await browser.releaseActions(); }
      const sorting = await checkpoint(browser);
      await fill(browser, SEARCH, "sort:name");
      // The sort's own search has completed and rendered before the Pane's state is taken.
      await commandsRendered(browser, sorting);
      await expand(browser, "other  master");
      await expand(browser, "repo  master");
      const shown = await layout();
      const before = await held();
      assert.equal(before.query, "sort:name");
      assert.ok(shown.pane !== null && shown.pane > 0 && shown.divider, `the Pane and its divider shown: ${JSON.stringify(shown)}`);
      assert.equal(await toggle().getAttribute("aria-pressed"), "true", "the switch reads as on");
      const title = await mainTitle(browser);
      const view = await camera(browser);

      // Ctrl+B with the keyboard in the Pane hides it: the Main area as wide as the window, its
      // view and camera as they were, and the keyboard there.
      await waitFor(browser, () => browser.execute(() => !!document.activeElement?.closest('section[aria-label="Pane"]')), "the keyboard in the Pane");
      await press(browser, [primaryModifier, "b"], async () => (await layout()).pane === null, "Ctrl+B hid the Pane");
      const hidden = await layout();
      assert.ok(!hidden.divider && Math.abs(hidden.main - hidden.window) <= 1, `the Main area takes the window's width: ${JSON.stringify(hidden)}`);
      assert.equal(await toggle().getAttribute("aria-pressed"), "false", "the switch reads as off");
      await waitFor(browser, () => mainFocused(browser), "the keyboard moved to the Main area");
      assert.equal(await mainTitle(browser), title, "the same view");
      assert.equal(await camera(browser), view, "the graph neither zoomed nor panned");
      // The graph and its Detail work whole with the Pane hidden; Esc closes the Detail, and Esc
      // again goes back to the Pane, shown first as it was, with the keyboard on its entries.
      await openArtifact(browser, "a:release", "Artifact release");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      await press(browser, ["Escape"], async () => (await layout()).pane !== null && await entriesFocused(browser), "Esc showed the Pane, the keyboard on its entries");
      const back = await layout();
      assert.ok(back.divider && Math.abs(back.pane! - shown.pane!) <= 1, `the Pane at the width it had: ${JSON.stringify({ shown, back })}`);
      assert.deepEqual(await held(), before, "its search, selection and expanded branches as they were");

      // The switch hides it and shows it again, as it was.
      await toggle().click();
      await waitFor(browser, async () => (await layout()).pane === null, "the switch hid the Pane");
      await toggle().click();
      await waitFor(browser, async () => (await layout()).pane !== null, "the switch showed the Pane");
      const again = await layout();
      assert.ok(again.divider && Math.abs(again.pane! - shown.pane!) <= 1, `the Pane at the width it had: ${JSON.stringify({ shown, again })}`);
      assert.equal(await toggle().getAttribute("aria-pressed"), "true", "the switch reads as on again");
      assert.deepEqual(await held(), before, "its search, selection and expanded branches as they were again");

      // The palette's command hides it too; / shows it first and focuses its search.
      await press(browser, [primaryModifier, "k"], () => browser.execute(() => !!document.querySelector('[aria-label="Command palette"][data-state="open"]')), "the palette open");
      await choosePaletteCommand(browser, "Show or hide the Pane");
      await press(browser, ["Enter"], async () => (await layout()).pane === null, "the palette's command hid the Pane");
      await press(browser, ["/"], async () => (await layout()).pane !== null && await browser.$(SEARCH).isFocused(), "/ showed the Pane and focused its search");

      // A waiting review used with the Pane hidden: Esc leaves it back to the Pane, shown again
      // with the review's entry selected and the keyboard on the entries.
      const id = signoff(fixture).id;
      await openWaiting(browser, id);
      await browser.$('[aria-label="Human review"]').click();
      await press(browser, [primaryModifier, "b"], async () => (await layout()).pane === null, "Ctrl+B hid the Pane over the review");
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice"
        && await reviewing(browser, '[aria-label="Human review"]'), "claimed with the Pane hidden, the review showing it as yours");
      await press(browser, ["Escape"], async () => (await layout()).pane !== null && await entriesFocused(browser), "Esc left the review back to the Pane");
      assert.ok((await layout()).divider, "its divider back");
      assert.equal((await waiting(browser)).rows.find((row) => row.selected)?.id, id, "the review's entry stays selected");
    },
  ),
  // UX journey: J-HELP
  scenario<Record<string, never>>(
    "navigation-help-review",
    "lists the keys of a Human review with ? while it has focus, and closes them with Esc",
    async () => ({ fixture: await create(), args: ["review"], data: {} }),
    async (browser) => {
      const helpOpen = () => browser.$('[role="dialog"].help').isExisting();
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "the waiting review open at launch");
      await browser.$('[aria-label="Human review"]').click();
      await press(browser, ["?"], helpOpen, "the review's keys");
      const shown = await shortcuts(browser);
      for (const [keys, action] of [
        ["c", /^Claim to review$/], ["u", /^Release the claim$/], ["g / r", /^Open the GREEN or the RED form$/],
        [shortcutLabel("Ctrl+S"), /^Submit$/], ["t", /^Show or hide Debug$/], [shortcutLabel("Ctrl+K"), /^Command palette$/],
      ] as const) {
        assert.ok(shown.some((entry) => entry.keys === keys && action.test(entry.action)), `the review's Help pairs ${keys} with ${action}: ${JSON.stringify(shown)}`);
      }
      assert.ok(!shown.some((entry) => entry.keys === "m"), "no graph keys");
      assert.deepEqual(await helpMarks(browser), [], "no marks for a review");
      await press(browser, ["Escape"], async () => !(await helpOpen()), "the review's keys closed");
    },
  ),
  // UX journeys: J-HELP J-MAIN-CURRENT
  scenario<Record<string, never>>(
    "navigation-help-current",
    "pairs each of a Current's marks with what verify would do, with ? on its graph",
    async () => {
      const fixture = createGraph();
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const helpOpen = () => browser.$('[role="dialog"].help').isExisting();
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown");
      await graphShown(browser, "a:release");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await press(browser, ["?"], helpOpen, "the Current's keys and marks");
      assert.deepEqual(await helpMarks(browser), CURRENT_MARKS, "what verify would do, each mark with its word");
      const shown = await shortcuts(browser);
      assert.ok(shown.some((entry) => entry.keys === "m" && /^Minimize or expand a whole$/.test(entry.action)), `the graph's keys: ${JSON.stringify(shown)}`);
      await press(browser, ["Escape"], async () => !(await helpOpen()), "the Current's keys closed");
    },
  ),
];
