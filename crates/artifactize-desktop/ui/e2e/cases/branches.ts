// The Pane's Branches and Waiting stack in the real app on CLI-made state: a branch's chip row
// of Runs ending with the Run in progress or its Current, dragged sideways to bring older Runs;
// worktrees and waiting reviews that come and go in another process; and a review left for a
// branch without being closed.
import assert from "node:assert/strict";
import {
  addWaiting, addWorktrees, artifactize, changeNotes, create, createChain, createHistory, requests, signOff, signoff, verifyWaiting,
} from "../fixture.ts";
import { git } from "../environment.ts";
import { reviewing } from "../human-review.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativePath, removeTree } from "../platform.ts";
import { graphShown } from "../graph-view.ts";
import { branch, branches, chooseChip, listed, mainTitle, openWaiting, selectBranch, showRun, waiting } from "../pane.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

const ROW = '[role="group"][aria-label="Runs of repo master"]';

/** The chip row's scroll position and fades as drawn. */
function row(browser: WebdriverIO.Browser) {
  return browser.execute((selector) => {
    const element = document.querySelector<HTMLElement>(selector)!;
    const box = element.getBoundingClientRect();
    return {
      left: Math.round(element.scrollLeft),
      right: Math.round(element.scrollWidth - element.clientWidth - element.scrollLeft),
      overflows: element.scrollWidth > element.clientWidth,
      // A scrollbar takes height from the row's content box; this row has none.
      scrollbar: element.offsetHeight - element.clientHeight > 1 || getComputedStyle(element).scrollbarWidth !== "none",
      fadeLeft: element.dataset.fadeLeft === "true",
      fadeRight: element.dataset.fadeRight === "true",
      masked: getComputedStyle(element).maskImage !== "none",
      at: { x: Math.round(box.left + 12), y: Math.round(box.top + box.height / 2) },
    };
  }, ROW);
}

/** Hold the mouse on the row at its left end, drag it right to the window's edge and release
 * it; again, until `until` holds. A person's hand does the same strokes. */
async function drag(browser: WebdriverIO.Browser, until: () => Promise<boolean>) {
  const STROKES = 6;
  for (let stroke = 0; stroke < STROKES && !(await until()); stroke += 1) {
    const { at } = await row(browser);
    const width = await browser.execute(() => innerWidth);
    await browser.action("pointer", { id: "chips" }).move({ x: at.x, y: at.y }).down().perform(true);
    try {
      for (let x = at.x + 60; x < width - 4; x += 60) await browser.action("pointer", { id: "chips" }).move({ x, y: at.y }).perform(true);
      await browser.action("pointer", { id: "chips" }).up().perform();
    } finally { await browser.releaseActions(); }
  }
  await waitFor(browser, until, "the dragged row's outcome");
}

export const scenarios: Scenario[] = [
  // UX journey: J-BRANCH-RUNS; UX edges: E-PANE-SELECT E-PANE-DRAG E-PANE-CHIP E-CURRENT-CHIP
  scenario<{ runs: string[] }>(
    "branches-chip-row",
    "lays a branch's Runs out oldest to newest ending with its Current, drags the row for older Runs without choosing one, and shows a clicked chip",
    async () => {
      const { fixture, runs } = createHistory(24);
      return { fixture, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      await listed(browser, "repo  master");
      // Selecting the branch shows its last chip: with no Run in progress, its Current.
      await selectBranch(browser, "repo  master");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the branch's Current shown");
      let shown = (await branch(browser, "repo  master"))!;
      // The newest twenty Runs, oldest to newest, then the Current chip, filled.
      assert.deepEqual(shown.chips.map((chip) => chip.id), [...runs.slice(-20), "current"]);
      assert.deepEqual(shown.chips.map((chip) => chip.filled), [...Array(20).fill(false), true]);
      for (const chip of shown.chips.slice(0, -1)) {
        assert.equal(chip.glyph, "✓", `${chip.id} shows its status`);
        assert.match(chip.text, /^GREEN \d+[smhd]/, `${chip.id} reads its status and age`);
      }
      // The row starts at its newest end with no scrollbar; the chips fade out at the end with more.
      let drawn = await row(browser);
      assert.ok(drawn.overflows && !drawn.scrollbar, `the row is wider than its place and has no scrollbar: ${JSON.stringify(drawn)}`);
      assert.ok(drawn.right <= 1 && drawn.fadeLeft && !drawn.fadeRight && drawn.masked, `the newest end shown, older chips fading: ${JSON.stringify(drawn)}`);

      // Dragging it right brings older chips into view, and at its oldest end older Runs arrive
      // without moving what is shown.
      await drag(browser, async () => (await branch(browser, "repo  master"))?.chips.length === 25);
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.map((chip) => chip.id), [...runs, "current"], "every Run of the branch, oldest first");
      drawn = await row(browser);
      assert.ok(drawn.right > 1 && drawn.fadeRight, `the row scrolled away from its newest end, newer chips fading: ${JSON.stringify(drawn)}`);
      await drag(browser, async () => (await row(browser)).left <= 1);
      drawn = await row(browser);
      assert.ok(!drawn.fadeLeft && drawn.fadeRight, `the oldest end shown, newer chips fading: ${JSON.stringify(drawn)}`);
      // No drag chose a chip: the Current is still the one shown.
      assert.equal(await mainTitle(browser), "Current · repo  master", "a drag chooses nothing");
      assert.equal((await branch(browser, "repo  master"))?.chips.at(-1)?.filled, true);

      // A click on a chip shows that Run, filled.
      await showRun(browser, "repo  master", runs[0]);
      await graphShown(browser, "a:n000");
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.filter((chip) => chip.filled).map((chip) => chip.id), [runs[0]], "only the clicked chip is filled");
      // The Current chip shows the Current again.
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown again");
    },
  ),

  // UX journeys: J-BRANCH-RUNS J-LIVE; UX edges: E-BRANCH-RUNNING E-BRANCH-RUN-END
  scenario<{ first: string }>(
    "branches-run-in-progress",
    "ends a branch's row with a Run another process starts, running its time so far, and with the Current again once it ends",
    async () => {
      const fixture = await create();
      const first = signoff(fixture);
      signOff(fixture, first.id);
      return { fixture, data: { first: first.runId } };
    },
    async (browser, { fixture, data: { first } }) => {
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([first, "current"]),
        "the ended Run, then the Current");
      // Another process starts a Run that waits for a person.
      changeNotes(fixture, "# Release notes\n- A second release.\n");
      const run = await verifyWaiting(fixture);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([first, run]),
        "the row ends with the Run in progress, the Current chip gone");
      const running = (await branch(browser, "repo  master"))!.chips[1];
      assert.match(running.text, /^Running \d+[smh]/, `the running chip reads its time so far: ${running.text}`);
      // Its sign-off is given elsewhere: the Run ends, its chip shows its final status and age.
      const request = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "release/signoff");
      assert.ok(request, "the new Run asks for the sign-off");
      signOff(fixture, request.id);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([first, run, "current"]),
        "the row ends with the Current again");
      const ended = (await branch(browser, "repo  master"))!.chips[1];
      assert.equal(ended.glyph, "✓");
      assert.match(ended.text, /^GREEN \d+[smhd]/, `the ended chip reads its final status and age: ${ended.text}`);
    },
  ),

  // UX journeys: J-BRANCHES J-LIVE J-WAITING; UX edge: E-PANE-WORKTREES
  scenario<{ runs: string[] }>(
    "branches-worktrees",
    "lists worktrees another process adds, a detached one by its folder, and drops removed or deleted ones while their waiting reviews stay",
    async () => {
      const fixture = await create();
      const runs = await addWorktrees(fixture);
      return { fixture, data: { runs } };
    },
    async (browser, { fixture, data: { runs } }) => {
      const names = async () => (await branches(browser)).map((entry) => entry.name).sort();
      await waitFor(browser, async () => (await names()).length === 3, "the three live branches");
      assert.deepEqual(await names(), ["repo  master", "repo  scope-one", "repo  scope-two"]);
      const reviews = requests(fixture).filter((request) => runs.includes(request.runId) && request.status === "WAITING_HUMAN").map((request) => request.id);
      assert.equal(reviews.length, 2, "each worktree's Run waits for a person");
      // Git alone, in another process: a new branch's worktree and a detached one appear
      // within seconds, without a reload.
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "added", nativePath(fixture.root, "added"));
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "--detach", nativePath(fixture.root, "loose"));
      await waitFor(browser, async () => (await names()).length === 5, "the added worktrees listed");
      assert.deepEqual(await names(), ["repo  added", "repo  loose", "repo  master", "repo  scope-one", "repo  scope-two"]);
      assert.deepEqual((await branch(browser, "repo  added"))?.chips.map((chip) => chip.id), ["current"], "a branch without Runs has its Current chip");
      // A worktree Git removes and one whose folder is deleted both go.
      git(fixture.repo, "-C", fixture.repo, "worktree", "remove", nativePath(fixture.root, "added"));
      removeTree(nativePath(fixture.root, "scope-one"));
      await waitFor(browser, async () => (await names()).length === 3, "the removed worktrees gone");
      assert.deepEqual(await names(), ["repo  loose", "repo  master", "repo  scope-two"]);
      // The deleted branch's waiting review stays in the Waiting stack.
      const stack = await waiting(browser);
      for (const id of reviews) assert.ok(stack.rows.some((entry) => entry.id === id), `${id} still waits`);
      assert.ok(stack.rows.some((entry) => entry.where === "repo  scope-one"), "with its removed branch named");
    },
  ),

  // UX journeys: J-WAITING J-LIVE; UX edges: E-WAITING-CHANGE E-REVIEW-OPEN
  scenario<Record<string, never>>(
    "branches-waiting-change",
    "adds a review that starts waiting elsewhere to the stack, opens it on a click, and drops it once submitted elsewhere",
    async () => ({ fixture: createChain(1), data: {} }),
    async (browser, { fixture }) => {
      await listed(browser, "repo  master");
      const height = () => browser.execute(() => Math.round(document.querySelector('section[aria-label="Waiting"]')!.getBoundingClientRect().height));
      let stack = await waiting(browser);
      assert.deepEqual(stack, { bar: "Waiting (0)", rows: [] }, "a thin bar with none waiting");
      const empty = await height();
      // Another repository's verify asks for a person.
      const [id] = await addWaiting(fixture, "gates", 1);
      await waitFor(browser, async () => (await waiting(browser)).rows.length === 1, "the new review listed");
      stack = await waiting(browser);
      assert.equal(stack.bar, "Waiting (1)");
      assert.deepEqual(stack.rows.map((entry) => [entry.id, entry.eval, entry.where]), [[id, "gate00/signoff", "gates  master"]]);
      assert.match(stack.rows[0].meta, /^\d+[smh]\S* · -$/, `how long it waits and that nobody claimed it: ${stack.rows[0].meta}`);
      assert.ok((await height()) > empty, "the stack grew upward by its row");
      // A click opens it in the Main area.
      await openWaiting(browser, id);
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed`),
        "the clicked review open");
      // Submitted elsewhere, it stops waiting: its row goes and the bar reads (0) again.
      signOff(fixture, id, "{}");
      await waitFor(browser, async () => (await waiting(browser)).bar === "Waiting (0)", "the submitted review dropped");
      assert.deepEqual((await waiting(browser)).rows, []);
    },
  ),

  // UX journey: J-WAITING
  scenario<{ ids: string[] }>(
    "branches-waiting-stack",
    "grows the Waiting stack upward with each review up to half of the Pane, beyond which it scrolls",
    async () => {
      const fixture = createChain(1);
      const ids = await addWaiting(fixture, "gates", 16);
      return { fixture, data: { ids } };
    },
    async (browser, { data: { ids } }) => {
      await waitFor(browser, async () => (await waiting(browser)).rows.length === ids.length, "every waiting review listed");
      const drawn = await browser.execute(() => {
        const pane = document.querySelector('section[aria-label="Pane"]')!.getBoundingClientRect();
        const stack = document.querySelector('section[aria-label="Waiting"]')!.getBoundingClientRect();
        const list = document.querySelector<HTMLElement>('[aria-label="Waiting reviews"]')!;
        return { pane: pane.height, stack: stack.height, bottom: pane.bottom - stack.bottom, scrolls: list.scrollHeight > list.clientHeight };
      });
      assert.ok(drawn.stack <= drawn.pane / 2 + 1 && drawn.stack > drawn.pane / 2 - 40, `the stack takes half of the Pane: ${JSON.stringify(drawn)}`);
      assert.ok(drawn.bottom <= 1, "it sits at the bottom of the Pane");
      assert.ok(drawn.scrolls, "beyond half of the Pane its list scrolls");
    },
  ),

  // UX journey: J-MAIN-REVIEW; UX edges: E-REVIEW-OPEN E-MAIN-REVIEW-LEAVE
  scenario<{ run: string }>(
    "branches-review-leave",
    "leaves a review opened from the Pane for a branch's chip without closing it, and opens it again as it was left",
    async () => {
      const fixture = await create();
      return { fixture, data: { run: signoff(fixture).runId } };
    },
    async (browser, { fixture, data: { run } }) => {
      const id = signoff(fixture).id;
      await openWaiting(browser, id);
      const review = () => browser.$('[aria-label="Human review"]');
      await waitFor(browser, async () => (await review().$(".meta").getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed`), "the review open");
      // Its eval and Artifact read above it.
      assert.equal(await browser.$('section.panel[aria-label="Main area"] .crumb').getText(), "release/signoff · A person approves the release · Artifact release");
      await review().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "claimed");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      await fill(browser, "#field-comment", "half-written note");
      // A branch's chip shows its Run; the review is left, not closed: its claim stays.
      await showRun(browser, "repo  master", run);
      await graphShown(browser, "a:release");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "leaving the review kept its claim");
      assert.equal(artifactize(fixture, "request", "show", id), 0);
      // Opened again, it is as it was left: claimed, its form and draft open.
      await openWaiting(browser, id);
      await browser.$("#field-comment").waitForExist({ timeout: 15000, timeoutMsg: "the form as it was left" });
      assert.equal(await browser.$("#field-comment").getValue(), "half-written note");
      assert.match(await review().$(".meta").getText(), /claimed by you \(alice\)/);
    },
  ),
];
