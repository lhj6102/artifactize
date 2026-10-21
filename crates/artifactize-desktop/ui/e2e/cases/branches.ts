// The Pane's Branches and Waiting stack in the real app on CLI-made state: a branch's chip row
// of Runs ending with the Run in progress or its Current, dragged sideways to bring older Runs;
// worktrees and waiting reviews that come and go in another process; and a review left for a
// branch without being closed.
import assert from "node:assert/strict";
import {
  addFolder, addWaiting, addWorktrees, artifactize, changeNotes, create, createChain, createHistory, createSharedBranch, ended, orderRuns, requests, signOff, signoff,
  verifyWaiting,
} from "../fixture.ts";
import { git } from "../environment.ts";
import { reviewing } from "../human-review.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { exists, nativePath, readText, removeTree, type NativePath } from "../platform.ts";
import { graphShown } from "../graph-view.ts";
import { runOf } from "../tool-log.ts";
import { branch, branches, chooseChip, entriesFocused, listed, mainTitle, openWaiting, selectBranch, showRun, waiting, leans, type Chip } from "../pane.ts";

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
      end: { x: Math.round(box.right - 12), y: Math.round(box.top + box.height / 2) },
    };
  }, ROW);
}

/** Hold the mouse on the row at its right end, drag it left to the window's edge, bringing older
 * Runs in from the right, and release it; again, until `until` holds. A person's hand does the
 * same strokes. */
/** The starred branches saved in `state`, or null before the first star. The file is replaced
 * whole, never written in place. */
function kept(state: NativePath): unknown[] | null {
  const file = nativePath(state, "starred-branches.json");
  return exists(file) ? JSON.parse(readText(file)) as unknown[] : null;
}

async function drag(browser: WebdriverIO.Browser, until: () => Promise<boolean>) {
  // A stroke spans about the Pane's width, so a long row takes many.
  const STROKES = 16;
  for (let stroke = 0; stroke < STROKES && !(await until()); stroke += 1) {
    const { end: at } = await row(browser);
    await browser.action("pointer", { id: "chips" }).move({ x: at.x, y: at.y }).down().perform(true);
    try {
      for (let x = at.x - 60; x > 4; x -= 60) await browser.action("pointer", { id: "chips" }).move({ x, y: at.y }).perform(true);
      await browser.action("pointer", { id: "chips" }).up().perform();
    } finally { await browser.releaseActions(); }
  }
  await waitFor(browser, until, "the dragged row's outcome");
}

export const scenarios: Scenario[] = [
  // UX journey: J-BRANCH-RUNS; UX edges: E-PANE-SELECT E-PANE-DRAG E-PANE-CHIP E-CURRENT-CHIP
  scenario<{ runs: string[] }>(
    "branches-chip-row",
    "lays a branch's Runs out newest to oldest after its Current, drags the row for older Runs without choosing one, and shows a clicked chip",
    async () => {
      const { fixture, runs } = createHistory(24);
      return { fixture, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      const newest = [...runs].reverse();
      await listed(browser, "repo  master");
      // Selecting the branch shows its first chip: with no Run in progress, its Current.
      await selectBranch(browser, "repo  master");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the branch's Current shown");
      let shown = (await branch(browser, "repo  master"))!;
      // The Current chip, filled, then the newest Runs, newest first; older Runs arrive later.
      const first = shown.chips.length - 1;
      assert.ok(first > 0 && first < runs.length, `some of the newest Runs first: ${first} of ${runs.length}`);
      assert.deepEqual(shown.chips.map((chip) => chip.id), ["current", ...newest.slice(0, first)]);
      assert.deepEqual(shown.chips.map((chip) => chip.filled), [true, ...Array(first).fill(false)]);
      for (const chip of shown.chips.slice(1)) {
        assert.equal(chip.glyph, "✓", `${chip.id} shows its status`);
        assert.match(chip.text, /^GREEN \d+[smhd]/, `${chip.id} reads its status and age`);
        // Tinted with its status colour, its glyph in that colour.
        assert.ok(leans(chip.paint, 1) && leans(chip.ink, 1), `${chip.id} is tinted green, its glyph green: ${JSON.stringify(chip)}`);
      }
      // The shown Current chip is filled with the accent.
      assert.ok(leans(shown.chips[0].paint, 2), `the shown Current chip is filled with the accent: ${JSON.stringify(shown.chips[0])}`);
      // The row starts at its newest end with no scrollbar; the chips fade out at the end with more.
      let drawn = await row(browser);
      assert.ok(drawn.overflows && !drawn.scrollbar, `the row is wider than its place and has no scrollbar: ${JSON.stringify(drawn)}`);
      assert.ok(drawn.left <= 1 && !drawn.fadeLeft && drawn.fadeRight && drawn.masked, `the newest end shown, older chips fading: ${JSON.stringify(drawn)}`);

      // Dragging it left brings older chips into view, and at its oldest end older Runs arrive
      // without moving what is shown.
      await drag(browser, async () => (await branch(browser, "repo  master"))?.chips.length === 25);
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.map((chip) => chip.id), ["current", ...newest], "every Run of the branch, newest first");
      drawn = await row(browser);
      assert.ok(drawn.left > 1 && drawn.fadeLeft, `the row scrolled away from its newest end, newer chips fading: ${JSON.stringify(drawn)}`);
      await drag(browser, async () => (await row(browser)).right <= 1);
      drawn = await row(browser);
      assert.ok(drawn.fadeLeft && !drawn.fadeRight, `the oldest end shown, newer chips fading: ${JSON.stringify(drawn)}`);
      // No drag chose a chip: the Current is still the one shown.
      assert.equal(await mainTitle(browser), "Current · repo  master", "a drag chooses nothing");
      assert.equal((await branch(browser, "repo  master"))?.chips[0]?.filled, true);

      // A click on a chip shows that Run, filled.
      await showRun(browser, "repo  master", runs[0]);
      await graphShown(browser, "a:n000");
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.filter((chip) => chip.filled).map((chip) => chip.id), [runs[0]], "only the clicked chip is filled");
      // Filled with its status colour, stronger than the tint of a chip not shown; the Current
      // chip, not shown, keeps an accent tint.
      const [filled, tinted] = [shown.chips.at(-1)!, shown.chips.at(-2)!];
      const strength = (chip: Chip) => chip.paint[1] - Math.max(chip.paint[0], chip.paint[2]);
      assert.ok(leans(filled.paint, 1) && strength(filled) > strength(tinted), `the shown Run's chip is filled green: ${JSON.stringify({ filled, tinted })}`);
      assert.ok(leans(shown.chips[0].paint, 2), `the Current chip keeps an accent tint: ${JSON.stringify(shown.chips[0])}`);
      // The Current chip shows the Current again.
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown again");
    },
  ),

  // UX journeys: J-BRANCH-RUNS J-LIVE; UX edges: E-BRANCH-RUNNING E-BRANCH-RUN-END
  scenario<{ first: string }>(
    "branches-run-in-progress",
    "starts a branch's row with a Run another process starts, running its time so far, and with the Current again once it ends",
    async () => {
      const fixture = await create();
      const first = signoff(fixture);
      signOff(fixture, first.id);
      assert.equal(ended(fixture, first.runId), "GREEN", "the first Run ended GREEN");
      return { fixture, data: { first: first.runId } };
    },
    async (browser, { fixture, data: { first } }) => {
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify(["current", first]),
        "the Current, then the ended Run");
      // Another process starts a Run that waits for a person.
      changeNotes(fixture, "# Release notes\n- A second release.\n");
      const run = await verifyWaiting(fixture);
      // Explicitly the newer, whatever times the two verifies were saved at.
      orderRuns(fixture, [first, run]);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([run, first]),
        "the row starts with the Run in progress, the Current chip gone");
      const running = (await branch(browser, "repo  master"))!.chips[0];
      assert.match(running.text, /^Running \d+[smh]/, `the running chip reads its time so far: ${running.text}`);
      assert.ok(leans(running.paint, 2), `the running chip is tinted blue: ${JSON.stringify(running)}`);
      // Its sign-off is given elsewhere: the Run ends, its chip shows its final status and age.
      const request = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "release/signoff");
      assert.ok(request, "the new Run asks for the sign-off");
      signOff(fixture, request.id);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify(["current", run, first]),
        "the row starts with the Current again");
      const ended = (await branch(browser, "repo  master"))!.chips[1];
      assert.equal(ended.id, run, "the ended Run follows the Current chip");
      assert.equal(ended.glyph, "✓");
      assert.match(ended.text, /^GREEN \d+[smhd]/, `the ended chip reads its final status and age: ${ended.text}`);
      assert.ok(leans(ended.paint, 1), `the GREEN chip is tinted green: ${JSON.stringify(ended)}`);
      // The chips carry the entry: its name is a smaller, lighter label above them.
      const type = await browser.execute(() => {
        const row = document.querySelector('[aria-label="Branches"] [role="option"]')!;
        const read = (element: Element) => ({ size: parseFloat(getComputedStyle(element).fontSize), weight: Number(getComputedStyle(element).fontWeight) });
        return { name: read(row.querySelector(".name")!), chip: read(row.querySelector("button[data-chip]")!) };
      });
      assert.ok(type.name.size < type.chip.size && type.name.weight < type.chip.weight, `the name is quieter than its chips: ${JSON.stringify(type)}`);
    },
  ),

  // UX journey: J-BRANCH-STAR; UX edge: E-PANE-STAR
  scenario<Record<string, never>>(
    "branches-star",
    "lists a starred branch first, keeps its star for the next monitor, and lists it with the others again once the star is taken away",
    async () => {
      const fixture = await create();
      await addWorktrees(fixture);
      return { fixture, data: {} };
    },
    async (browser, { fixture }) => {
      const names = async () => (await branches(browser)).map((entry) => entry.name);
      await waitFor(browser, async () => (await names()).length === 3, "the three worktrees listed");
      const before = await names();
      const last = before.at(-1)!;
      assert.deepEqual((await branches(browser)).map((entry) => [entry.star, entry.starred]), [["☆", false], ["☆", false], ["☆", false]], "no star yet");
      // Starred, the last branch is listed first, the others in their order after it.
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${last}`)}]`).click();
      await waitFor(browser, async () => (await names())[0] === last, `${last} listed first: ${JSON.stringify(await names())}`);
      assert.deepEqual(await names(), [last, ...before.slice(0, -1)]);
      const starred = (await branch(browser, last))!;
      assert.ok(starred.starred && starred.star === "★", `its star is on: ${JSON.stringify(starred)}`);
      // Kept in the state directory, for the next monitor of this user.
      await waitFor(browser, async () => kept(fixture.state)?.length === 1, "the star saved");
    },
    // The next monitor lists it first, starred; taken away, it is listed with the others again.
    async (browser, { fixture }) => {
      const names = async () => (await branches(browser)).map((entry) => entry.name);
      await waitFor(browser, async () => (await names()).length === 3, "the three worktrees listed again");
      const shown = await branches(browser);
      assert.deepEqual(shown.map((entry) => [entry.star, entry.starred]), [["★", true], ["☆", false], ["☆", false]], `the star kept: ${JSON.stringify(shown)}`);
      const starred = shown[0].name;
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${starred}`)}]`).click();
      // It was the last of the search's order before it was starred.
      const after = [...shown.slice(1).map((entry) => entry.name), starred];
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(after), `listed with the others: ${JSON.stringify(await names())}`);
      assert.equal((await branch(browser, starred))?.starred, false);
      await waitFor(browser, async () => kept(fixture.state)?.length === 0, "the star's removal saved");
    },
  ),

  // UX journeys: J-BRANCHES J-LIVE J-WAITING; UX edge: E-PANE-WORKTREES
  scenario<{ runs: string[]; plain: { folder: NativePath; run: string } }>(
    "branches-worktrees",
    "lists worktrees another process adds, a detached one by its folder, a folder outside Git by its name, and drops removed or deleted ones while their waiting reviews stay",
    async () => {
      const fixture = await create();
      const runs = await addWorktrees(fixture);
      const plain = addFolder(fixture, "loosefiles");
      return { fixture, data: { runs, plain } };
    },
    async (browser, { fixture, data: { runs, plain } }) => {
      const names = async () => (await branches(browser)).map((entry) => entry.name).sort();
      await waitFor(browser, async () => (await names()).length === 4, "the four live entries");
      assert.deepEqual(await names(), ["loosefiles", "repo  master", "repo  scope-one", "repo  scope-two"]);
      // A folder outside Git with a Run is listed by its name, and selecting it shows its Current.
      assert.deepEqual((await branch(browser, "loosefiles"))?.chips.map((chip) => chip.id), ["current", plain.run]);
      await selectBranch(browser, "loosefiles");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · loosefiles", "the folder's Current shown");
      await graphShown(browser, "a:plain");
      // Deleted, it goes.
      removeTree(plain.folder);
      await waitFor(browser, async () => (await names()).length === 3, "the deleted folder gone");
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
      // A worktree Git no longer lists goes too, though its folder and files stay.
      removeTree(nativePath(fixture.root, "scope-two", ".git"));
      git(fixture.repo, "-C", fixture.repo, "worktree", "prune");
      await waitFor(browser, async () => (await names()).length === 2, "the unlisted worktree gone");
      assert.deepEqual(await names(), ["repo  loose", "repo  master"]);
      assert.equal(exists(nativePath(fixture.root, "scope-two", "release", "notes.md")), true, "its folder and files stay");
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
      // The bottom review starts hidden below the list's box. Moving the selection down to it
      // with j scrolls the list until it shows, and Enter opens that review.
      const last = (await waiting(browser)).rows.at(-1)!.id;
      const visible = () => browser.execute((key) => {
        const list = document.querySelector<HTMLElement>('[aria-label="Waiting reviews"]')!;
        const box = list.getBoundingClientRect();
        const row = list.querySelector<HTMLElement>(`[data-key=${JSON.stringify(key)}]`)!.getBoundingClientRect();
        return row.top >= box.top - 1 && row.bottom <= box.bottom + 1;
      }, last);
      assert.equal(await visible(), false, "the bottom review starts out of view");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      const selected = async () => (await waiting(browser)).rows.find((row) => row.selected)?.id ?? null;
      // A bound on the moves through this fixture's two branches and sixteen reviews.
      const MOVES = 40;
      for (let move = 0; move < MOVES && (await selected()) !== last; move += 1) await browser.keys(["j"]);
      await waitFor(browser, async () => (await selected()) === last, "the bottom review selected");
      await waitFor(browser, visible, "the list scrolled the selected review into view");
      await press(browser, ["Enter"], async () => (await mainTitle(browser)) === "Review", "its Review shown");
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${last} · WAITING_HUMAN`), "the scrolled-to review open");
    },
  ),

  // UX journey: J-MAIN-REVIEW; UX edges: E-REVIEW-OPEN E-MAIN-REVIEW-LEAVE
  scenario<{ run: string }>(
    "branches-review-leave",
    "leaves a review opened from the Pane for a branch's chip without closing it, and opens it again as it was left, its Tool log included",
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
      // A tool run of the review, in its Tool log before the review is left.
      const REVIEW = '[aria-label="Human review"]';
      await review().$('[aria-label="Tools of release"]').$("button=▶ notes").waitForEnabled({ timeout: 15000, timeoutMsg: "the notes tool usable" });
      await review().$('[aria-label="Tools of release"]').$("button=▶ notes").click();
      await waitFor(browser, async () => (await runOf(browser, REVIEW, "notes"))?.at(-1)?.kind === "end", "the notes run ended in the review's Tool log");
      const logged = await runOf(browser, REVIEW, "notes");
      assert.deepEqual(logged, [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      // The tool's button let go of the keyboard while it ran: the instruction takes it.
      await review().$('[aria-label="Instruction"]').click();
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
      assert.deepEqual(await runOf(browser, REVIEW, "notes"), logged, "its Tool log kept the run made before it was left");
    },
  ),

  // UX journeys: J-BRANCHES J-BRANCH-RUNS; UX edge: E-PANE-CHIP
  scenario<{ master: string; shared: string[] }>(
    "branches-one-entry",
    "lists a branch once with the Runs made on it from every folder, by a person and by an agent session, and no entry per folder or session",
    async () => {
      const { fixture, master, shared } = createSharedBranch();
      return { fixture, data: { master, shared } };
    },
    async (browser, { data: { master, shared } }) => {
      await waitFor(browser, async () => (await branch(browser, "repo  shared"))?.chips.length === 3, "the shared branch's Runs listed");
      // One entry per project and branch: none for the removed folder, the live one or the session.
      assert.deepEqual((await branches(browser)).map((entry) => entry.name).sort(), ["repo  master", "repo  shared"]);
      // Its row holds the person's Run from the removed folder and the agent session's from the
      // live one after its Current, newest first.
      assert.deepEqual((await branch(browser, "repo  shared"))?.chips.map((chip) => chip.id), ["current", ...[...shared].reverse()]);
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id), ["current", master]);
      // A click on a chip of a branch not selected selects that branch and shows the chip's Run:
      // here the one from the removed folder, still the branch's.
      assert.deepEqual((await branches(browser)).filter((entry) => entry.selected), [], "no branch selected yet");
      await showRun(browser, "repo  shared", shared[0]);
      await graphShown(browser, "a:n000");
      assert.equal((await branch(browser, "repo  shared"))?.selected, true, "the chip's branch selected");
      assert.deepEqual((await branch(browser, "repo  shared"))?.chips.filter((chip) => chip.filled).map((chip) => chip.id), [shared[0]]);
      // Then a chip of the other branch: that branch is selected instead, its chip the one filled.
      await showRun(browser, "repo  master", master);
      assert.deepEqual((await branches(browser)).filter((entry) => entry.selected).map((entry) => entry.name), ["repo  master"], "only the other branch selected");
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.filter((chip) => chip.filled).map((chip) => chip.id), [master]);
      assert.deepEqual((await branch(browser, "repo  shared"))?.chips.filter((chip) => chip.filled), [], "the first branch's chip no longer filled");
    },
  ),
];
