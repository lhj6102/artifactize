// The Pane's Branches and Waiting stack in the real app on CLI-made state: a branch collapsed to
// its newest Run's status, expanded to its timeline of Runs led by the Run in progress or its
// Current, with Older runs bringing older ones; worktrees and waiting reviews that come and go
// in another process; and a review left for a branch without being closed.
import assert from "node:assert/strict";
import {
  addFolder, addWaiting, addWorktrees, artifactize, changeNotes, create, createChain, createHeld, createHistory, createSharedBranch, ended, newestRunStatus,
  orderRuns, requests, signOff, signoff, verifyWaiting,
} from "../fixture.ts";
import { git } from "../environment.ts";
import { reviewing } from "../human-review.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { exists, nativePath, readText, removeTree, writeText, type NativePath } from "../platform.ts";
import { graphShown } from "../graph-view.ts";
import { runOf } from "../tool-log.ts";
import { branch, branches, chooseChip, entriesFocused, expand, listed, mainTitle, openWaiting, runComment, SEARCH, selectBranch, showRun, waiting, leans } from "../pane.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The starred branches saved in `state`, or null before the first star. The file is replaced
 * whole, never written in place. */
function kept(state: NativePath): unknown[] | null {
  const file = nativePath(state, "starred-branches.json");
  return exists(file) ? JSON.parse(readText(file)) as unknown[] : null;
}

/** A comment longer than a timeline entry's line, as an agent working on a task gives it. */
const COMMENT = "Fixing the login form so that a wrong password says which field is wrong, then checking the error messages read the same on every platform";

/** The first row of the Branches: its chevron, and its Older runs button while it shows one. */
const FIRST = '[aria-label="Branches"] > [role="treeitem"]:first-child';

export const scenarios: Scenario[] = [
  // UX journey: J-LIVE; UX edges: E-BRANCH-RUNNING E-BRANCH-RUN-END
  scenario<{ gate: NativePath }>(
    "branches-run-executing",
    "starts a branch's timeline with a Run another process starts while its eval still runs, Running and its time so far, folded the same, and ends it",
    async () => {
      const fixture = createHeld();
      // No Run yet: the branch is listed because the monitor is opened on its repository.
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { gate: fixture.gate } };
    },
    async (browser, { fixture, data: { gate } }) => {
      await listed(browser, "repo  master");
      // Another process starts a Run whose only eval runs until the gate opens.
      assert.equal(artifactize(fixture, "verify", "--all"), 0);
      await waitFor(browser, () => newestRunStatus(fixture) === "RUNNING", "the Run is executing");
      const run = () => branch(browser, "repo  master");
      if ((await run())?.expanded) {
        await browser.$(`${FIRST} button.chevron`).click();
        await waitFor(browser, async () => (await run())?.expanded === false, "the branch folded");
      }
      await waitFor(browser, async () => /^Running \d+[smh]( \d+[smh])?$/.test((await run())?.latest ?? ""),
        "folded, the branch reads Running and its time so far");
      await browser.$(`${FIRST} button.chevron`).click();
      await waitFor(browser, async () => (await run())?.expanded === true, "the branch expanded");
      const shown = (await run())!;
      assert.ok(!shown.chips.some((chip) => chip.id === "current"), `the Current gone while the Run runs: ${JSON.stringify(shown.chips)}`);
      assert.match(shown.chips[0].text, /^Running \d+[smh]( \d+[smh])?$/, `the open entry reads its time so far: ${shown.chips[0].text}`);
      // The gate opens: the eval ends, the Run ends GREEN and the Current leads again.
      writeText(gate, "open");
      await waitFor(browser, () => newestRunStatus(fixture) === "GREEN", "the Run ended GREEN");
      await waitFor(browser, async () => (await run())?.chips[0]?.id === "current", "the timeline starts with the Current again");
      assert.match((await run())!.chips[1].text, /^GREEN \d+[smhd]( \d+[smh])? ago$/);
    },
  ),
  // UX journeys: J-BRANCH-RUNS J-MAIN-RUN; UX edges: E-PANE-FOLD E-PANE-SELECT E-PANE-OLDER E-PANE-CHIP E-CURRENT-CHIP
  scenario<{ runs: string[] }>(
    "branches-timeline",
    "folds a branch to its newest Run's status, expands it on a click to its Runs newest first after its Current, each with its comment cut short to one line, brings older Runs with Older runs, and shows a clicked entry with its comment in full",
    async () => {
      const { fixture, runs } = createHistory(24, COMMENT);
      return { fixture, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      const newest = [...runs].reverse();
      await listed(browser, "repo  master");
      // Folded, the branch reads `project / branch` over its newest Run's status and age.
      if ((await branch(browser, "repo  master"))?.expanded) {
        await browser.$(`${FIRST} button.chevron`).click();
        await waitFor(browser, async () => (await branch(browser, "repo  master"))?.expanded === false, "the branch folded");
      }
      let shown = (await branch(browser, "repo  master"))!;
      assert.equal(shown.label, "repo / master");
      assert.match(shown.latest, /^GREEN \d+[smhd]( \d+[smh])? ago$/, `the newest Run's status and age: ${shown.latest}`);
      assert.deepEqual(shown.chips, [], "no timeline while folded");
      // A click on it expands it and shows its newest entry: with no Run in progress, its Current.
      await selectBranch(browser, "repo  master");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the branch's Current shown");
      shown = (await branch(browser, "repo  master"))!;
      assert.equal(shown.latest, "", "expanded, the timeline says it instead");
      // The Current, shown, then the newest Runs, newest first; older Runs arrive later.
      const first = shown.chips.length - 1;
      assert.ok(first > 0 && first < runs.length, `some of the newest Runs first: ${first} of ${runs.length}`);
      assert.deepEqual(shown.chips.map((chip) => chip.id), ["current", ...newest.slice(0, first)]);
      assert.deepEqual(shown.chips.map((chip) => chip.filled), [true, ...Array(first).fill(false)]);
      assert.deepEqual([shown.chips[0].text, shown.chips[0].sub], ["Current", "Working tree"]);
      for (const chip of shown.chips.slice(1)) {
        assert.match(chip.text, /^GREEN \d+[smhd]( \d+[smh])? ago$/, `${chip.id} reads its status and age`);
        assert.equal(chip.sub, chip.id, `${chip.id} names its Run`);
        assert.ok(leans(chip.dot, 1), `${chip.id}'s dot is green: ${JSON.stringify(chip)}`);
      }
      // The newest Run was requested with a comment: below its Run ID, on one line, cut short.
      const [commented, ...plain] = shown.chips.slice(1);
      assert.ok(commented.noteOneLine && commented.noteCut && COMMENT.startsWith(commented.note.replace(/…$/, "")),
        `the comment under its Run ID, cut short to one line: ${JSON.stringify(commented)}`);
      assert.deepEqual(plain.map((chip) => chip.note), plain.map(() => ""), "a Run without a comment shows none");
      // Drawn as a timeline: the Current a ring, a line joining each dot to the next, and a
      // grey pill around the shown entry only.
      const look = await browser.execute((first: string) => {
        const clear = (colour: string) => colour === "transparent" || /^rgba\([^)]*,\s*0\s*\)$/.test(colour);
        const items = [...document.querySelectorAll<HTMLElement>(`${first} ol.timeline > li`)];
        const dots = items.map((item) => item.querySelector<HTMLElement>(".dot")!);
        const ring = getComputedStyle(dots[0]);
        return {
          ring: ring.boxShadow.includes("inset") && ring.backgroundColor !== getComputedStyle(dots[1]).backgroundColor,
          lines: items.slice(0, -1).map((item, at) => {
            const line = getComputedStyle(item, "::before");
            const centre = (box: DOMRect) => box.left + box.width / 2;
            const from = dots[at].getBoundingClientRect();
            const to = dots[at + 1].getBoundingClientRect();
            const top = item.getBoundingClientRect().top + Number.parseFloat(line.top);
            return line.content !== "none" && Number.parseFloat(line.width) >= 1 && !clear(line.backgroundColor) &&
              Math.abs(item.getBoundingClientRect().left + Number.parseFloat(line.left) + Number.parseFloat(line.width) / 2 - centre(from)) <= 1.5 &&
              top <= from.bottom + 2 && top + Number.parseFloat(line.height) >= to.top - 2;
          }),
          pills: items.map((item) => !clear(getComputedStyle(item.querySelector(".entry")!).backgroundColor)),
        };
      }, FIRST);
      assert.ok(look.ring, "the Current's dot is a ring, unlike a Run's light");
      assert.ok(look.lines.length > 0 && look.lines.every(Boolean), `a line joins each dot to the next: ${JSON.stringify(look.lines)}`);
      assert.deepEqual(look.pills, [true, ...Array(first).fill(false)], "a grey pill around the shown entry only");

      // Older runs brings the older Runs in below the oldest, without changing what is shown.
      for (let click = 0; click < 8 && (await branch(browser, "repo  master"))!.chips.length < runs.length + 1; click += 1) {
        const count = (await branch(browser, "repo  master"))!.chips.length;
        await browser.$(`${FIRST} button.older`).click();
        await waitFor(browser, async () => (await branch(browser, "repo  master"))!.chips.length > count, "older Runs arrived");
      }
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.map((chip) => chip.id), ["current", ...newest], "every Run of the branch, newest first");
      assert.equal(await browser.$(`${FIRST} button.older`).isExisting(), false, "no Older runs once every Run is listed");
      assert.equal(await mainTitle(browser), "Current · repo  master", "Older runs chooses nothing");

      // A click on the commented Run shows its comment in full under its headline.
      await showRun(browser, "repo  master", newest[0]);
      await waitFor(browser, async () => (await runComment(browser))?.text === COMMENT, "the Run's comment under its headline");
      assert.ok((await runComment(browser))!.whole, "the whole comment is drawn, not cut short");
      // A click on an entry shows that Run, the only one marked; without a comment it shows none.
      await showRun(browser, "repo  master", runs[0]);
      assert.equal(await runComment(browser), null, "a Run without a comment shows none");
      await graphShown(browser, "a:n000");
      shown = (await branch(browser, "repo  master"))!;
      assert.deepEqual(shown.chips.filter((chip) => chip.filled).map((chip) => chip.id), [runs[0]], "only the clicked entry is marked");
      // The Current shows the Current again.
      await chooseChip(browser, "repo  master", "current");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · repo  master", "the Current shown again");
      // The chevron folds it back to its status; what is shown stays.
      await browser.$(`${FIRST} button.chevron`).click();
      await waitFor(browser, async () => (await branch(browser, "repo  master"))?.expanded === false, "folded again");
      assert.match((await branch(browser, "repo  master"))!.latest, /^GREEN /);
      assert.equal(await mainTitle(browser), "Current · repo  master", "folding chooses nothing");
    },
  ),

  // UX journeys: J-BRANCH-RUNS J-LIVE; UX edges: E-BRANCH-RUNNING E-BRANCH-RUN-END
  scenario<{ first: string }>(
    "branches-run-in-progress",
    "starts a branch's timeline with a Run another process starts, running its time so far, and with the Current again once it ends",
    async () => {
      const fixture = await create();
      const first = signoff(fixture);
      signOff(fixture, first.id);
      assert.equal(ended(fixture, first.runId), "GREEN", "the first Run ended GREEN");
      return { fixture, data: { first: first.runId } };
    },
    async (browser, { fixture, data: { first } }) => {
      await expand(browser, "repo  master");
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify(["current", first]),
        "the Current, then the ended Run");
      // Another process starts a Run that waits for a person.
      changeNotes(fixture, "# Release notes\n- A second release.\n");
      const run = await verifyWaiting(fixture);
      // Explicitly the newer, whatever times the two verifies were saved at.
      orderRuns(fixture, [first, run]);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([run, first]),
        "the timeline starts with the Run in progress, the Current gone");
      const running = (await branch(browser, "repo  master"))!.chips[0];
      assert.match(running.text, /^Waiting for sign-off \d+[smh]( \d+[smh])?$/, `the open entry reads its time so far, not a time ago: ${running.text}`);
      assert.equal(running.sub, run);
      // Its sign-off is given elsewhere: the Run ends, its entry shows its final status and age.
      const request = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "release/signoff");
      assert.ok(request, "the new Run asks for the sign-off");
      signOff(fixture, request.id);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify(["current", run, first]),
        "the timeline starts with the Current again");
      const ended = (await branch(browser, "repo  master"))!.chips[1];
      assert.equal(ended.id, run, "the ended Run follows the Current");
      assert.match(ended.text, /^GREEN \d+[smhd]( \d+[smh])? ago$/, `the ended entry reads its final status and age: ${ended.text}`);
      assert.ok(leans(ended.dot, 1), `the GREEN entry's dot is green: ${JSON.stringify(ended)}`);
      // The branch's name reads first: its branch in bold, its project quieter beside it.
      const type = await browser.execute(() => {
        const row = document.querySelector('[aria-label="Branches"] > [role="treeitem"]')!;
        const read = (element: Element) => ({ weight: Number(getComputedStyle(element).fontWeight), colour: getComputedStyle(element).color });
        return { branch: read(row.querySelector(".branch-name")!), project: read(row.querySelector(".project")!) };
      });
      assert.ok(type.branch.weight > type.project.weight && type.branch.colour !== type.project.colour, `the branch reads before its project: ${JSON.stringify(type)}`);
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
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${(await branch(browser, last))!.label}`)}]`).click();
      await waitFor(browser, async () => (await names())[0] === last, `${last} listed first: ${JSON.stringify(await names())}`);
      assert.deepEqual(await names(), [last, ...before.slice(0, -1)]);
      const starred = (await branch(browser, last))!;
      assert.ok(starred.starred && starred.star === "★", `its star is on: ${JSON.stringify(starred)}`);
      // A second star: the starred branches keep the search's order among themselves, before
      // the others, with sort:recent and with sort:name alike.
      const second = before.at(-2)!;
      const unstarred = before[0];
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${(await branch(browser, second))!.label}`)}]`).click();
      const recent = [second, last, unstarred];
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(recent), `both starred first, in sort:recent order: ${JSON.stringify(await names())}`);
      const query = async (text: string) => {
        const read = await checkpoint(browser);
        await fill(browser, SEARCH, text);
        await commandsRendered(browser, read);
      };
      const byName = (list: string[]) => [...list].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
      await query("sort:name");
      const named = [...byName([second, last]), unstarred];
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(named), `both starred first, in sort:name order: ${JSON.stringify(await names())}`);
      await query("sort:recent");
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(recent), "sort:recent again");
      // Taking the second star away lists it with the others again.
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${(await branch(browser, second))!.label}`)}]`).click();
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify([last, ...before.slice(0, -1)]), `only ${last} starred again: ${JSON.stringify(await names())}`);
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
      await browser.$(`button[aria-label=${JSON.stringify(`Star ${shown[0].label}`)}]`).click();
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
      assert.equal((await branch(browser, "loosefiles"))?.label, "loosefiles");
      await expand(browser, "loosefiles");
      assert.deepEqual((await branch(browser, "loosefiles"))?.chips.map((chip) => chip.id), ["current", plain.run]);
      await selectBranch(browser, "loosefiles");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · loosefiles", "the folder's Current shown");
      await graphShown(browser, "a:plain");
      // Deleted, it goes.
      removeTree(plain.folder);
      await waitFor(browser, async () => (await names()).length === 3, "the deleted folder gone");
      assert.deepEqual(await names(), ["repo  master", "repo  scope-one", "repo  scope-two"]);
      const reviews = requests(fixture).filter((request) => runs.includes(request.runId) && request.status === "DEFERRED").map((request) => request.id);
      assert.equal(reviews.length, 2, "each worktree's Run waits for a person");
      // Git alone, in another process: a new branch's worktree and a detached one appear
      // within seconds, without a reload.
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "added", nativePath(fixture.root, "added"));
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "--detach", nativePath(fixture.root, "loose"));
      await waitFor(browser, async () => (await names()).length === 5, "the added worktrees listed");
      assert.deepEqual(await names(), ["repo  added", "repo  loose", "repo  master", "repo  scope-one", "repo  scope-two"]);
      assert.equal((await branch(browser, "repo  added"))?.latest, "No Runs yet", "folded, a branch without Runs says so");
      await expand(browser, "repo  added");
      assert.deepEqual((await branch(browser, "repo  added"))?.chips.map((chip) => chip.id), ["current"], "a branch without Runs has its Current");
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
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${id} · DEFERRED · unclaimed`),
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
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${last} · DEFERRED`), "the scrolled-to review open");
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
      await waitFor(browser, async () => (await review().$(".meta").getText()).startsWith(`Request ${id} · DEFERRED · unclaimed`), "the review open");
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
      await expand(browser, "repo  shared");
      await expand(browser, "repo  master");
      await waitFor(browser, async () => (await branch(browser, "repo  shared"))?.chips.length === 3, "the shared branch's Runs listed");
      // One entry per project and branch: none for the removed folder, the live one or the session.
      assert.deepEqual((await branches(browser)).map((entry) => entry.name).sort(), ["repo  master", "repo  shared"]);
      // Its timeline holds the person's Run from the removed folder and the agent session's from
      // the live one after its Current, newest first.
      assert.deepEqual((await branch(browser, "repo  shared"))?.chips.map((chip) => chip.id), ["current", ...[...shared].reverse()]);
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id), ["current", master]);
      // A click on an entry of a branch not selected selects it and shows the entry's Run:
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
