// The Pane's search on real state: several projects and worktrees share eval IDs. Its starting
// query is sort:recent; free text and key:value terms narrow the Branches and the Waiting stack
// alike, suggestions complete keys and values, Esc steps back to sort:recent and then to the
// entries, and the sort orders the Branches. A request that is no waiting review opens by ID
// read-only.
import assert from "node:assert/strict";
import { addRepository, addWorktrees, create, orderRuns, requests, signOff, signoff, type Fixture } from "../fixture.ts";
import { git } from "../environment.ts";
import { fill, press, typeText } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativePath, primaryModifier } from "../platform.ts";
import { branches, entriesFocused, mainTitle, SEARCH, searchFocused, waiting } from "../pane.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

const ALL = ["other  master", "repo  master", "repo  scope-one", "repo  scope-two", "third  master"];

/** The Run of the sign-off a repository added after `before` was read waits with. */
function addedRun(fixture: Fixture, before: Set<string>): string {
  const found = requests(fixture).find((request) => request.evalId === "release/signoff" && !before.has(request.id));
  assert.ok(found, "the added repository's own sign-off waits");
  return found.runId;
}

/** Five branches of three projects, each with a Run waiting for its sign-off, started in this
 * order: repo's master, its two worktrees, other's and third's. */
async function prepareSearch() {
  const fixture = await create();
  const runs = [signoff(fixture).runId, ...await addWorktrees(fixture)];
  for (const name of ["other", "third"]) {
    const before = new Set(requests(fixture).map((request) => request.id));
    await addRepository(fixture, name);
    runs.push(addedRun(fixture, before));
  }
  // Explicitly in that order, whatever times the verifies were saved at.
  orderRuns(fixture, runs);
  return { fixture, data: {} };
}

function search(browser: WebdriverIO.Browser) {
  const query = () => browser.$(SEARCH).getValue();
  const holds = (text: string) => async () => (await query()) === text;
  const suggestions = () =>
    browser.execute(() => [...document.querySelectorAll('[aria-label="Suggestions"] [role="option"]')].map((option) => option.textContent?.trim()));
  const names = async () => (await branches(browser)).map((entry) => entry.name);
  const where = async () => (await waiting(browser)).rows.map((row) => row.where);
  /** Both lists narrowed to exactly these branches. */
  const narrowed = (expected: string[]) => async () =>
    JSON.stringify([...await names()].sort()) === JSON.stringify([...expected].sort()) && JSON.stringify([...await where()].sort()) === JSON.stringify([...expected].sort());
  /** Set the whole query and wait for its own search to show. */
  const set = async (text: string) => {
    const before = await checkpoint(browser);
    await fill(browser, SEARCH, text);
    await commandsRendered(browser, before);
  };
  return { query, holds, suggestions, names, where, narrowed, set };
}

export const scenarios: Scenario[] = [
  // UX journey: J-PANE-SEARCH; UX edges: E-REVIEW-SEARCH-FOCUS E-REVIEW-SEARCH-TERMS E-REVIEW-SUGGEST
  // E-REVIEW-SUGGEST-CLOSE E-REVIEW-SEARCH-CLEAR E-REVIEW-SEARCH-LEAVE
  scenario<Record<string, never>>(
    "search-terms",
    "narrows the Branches and the Waiting stack alike by free text, repo: and branch:, takes suggestions, and steps back with Esc",
    prepareSearch,
    async (browser) => {
      const { query, holds, suggestions, names, narrowed, set } = search(browser);
      await waitFor(browser, narrowed(ALL), "every branch and every waiting review listed");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      // / focuses the search without typing a slash; its starting query is sort:recent.
      await press(browser, ["/"], () => searchFocused(browser), "the search focused");
      assert.equal(await query(), "sort:recent");
      // Free text: part of a branch, here of two worktrees' branches.
      await set("sort:recent scope");
      await waitFor(browser, narrowed(["repo  scope-one", "repo  scope-two"]), "both lists narrowed to the scope branches");
      // Free text also matches a waiting review's eval: every review waits for release/signoff,
      // while no project or branch is named so.
      await set("sort:recent signoff");
      await waitFor(browser, async () => (await names()).length === 0 && (await waiting(browser)).rows.length === ALL.length, "only the waiting reviews match their eval");
      // branch: matches part of the branch alone.
      await set("sort:recent branch:one");
      await waitFor(browser, narrowed(["repo  scope-one"]), "branch:one narrowed to scope-one");
      // Terms together keep only what every one of them matches, in both lists: repo's master
      // and the other projects are decoys that match one term but not the other.
      await set("sort:recent scope repo:repo");
      await waitFor(browser, narrowed(["repo  scope-one", "repo  scope-two"]), "free text and repo: together");
      await set("sort:recent repo:repo branch:two");
      await waitFor(browser, narrowed(["repo  scope-two"]), "repo: and branch: together");
      await set("sort:recent master repo:thi");
      await waitFor(browser, narrowed(["third  master"]), "free text and repo: together, other projects' master left out");
      // A term nobody matches empties both lists, whatever the other terms match.
      await set("sort:recent repo:repo branch:nothing");
      await waitFor(browser, narrowed([]), "no branch named nothing: both lists empty");
      // A key is suggested, then the projects as its values; all terms must match.
      await set("sort:recent ");
      await typeText(browser, "rep");
      await waitFor(browser, async () => (await suggestions()).includes("repo:"), "the repo key suggested");
      const chosen = () => browser.execute(() => document.querySelector('[aria-label="Suggestions"] [aria-selected="true"]')?.textContent?.trim() ?? null);
      await press(browser, ["ArrowDown"], async () => (await chosen()) === "repo:", "the repo key chosen", { handled: true });
      await press(browser, ["Enter"], holds("sort:recent repo:"), "the repo key taken");
      await waitFor(browser, async () => (await suggestions()).includes("repo:other"), "the projects as its values");
      await typeText(browser, "oth");
      await waitFor(browser, async () => JSON.stringify(await suggestions()) === JSON.stringify(["repo:other"]), "only the other project left");
      const beforeRepository = await checkpoint(browser);
      await press(browser, ["Tab"], holds("sort:recent repo:other "), "the project taken");
      await commandsRendered(browser, beforeRepository);
      await waitFor(browser, narrowed(["other  master"]), "repo:other narrowed both lists to its branch");
      // Esc closes the suggestions first, keeping the query.
      await typeText(browser, "bra");
      await waitFor(browser, async () => (await suggestions()).includes("branch:"), "the branch key suggested");
      await press(browser, ["Escape"], async () => (await suggestions()).length === 0, "the suggestions closed");
      assert.equal(await query(), "sort:recent repo:other bra", "the query is kept");
      // The next Esc puts sort:recent back, every branch and review listed again.
      const beforeClear = await checkpoint(browser);
      await press(browser, ["Escape"], holds("sort:recent"), "sort:recent put back");
      await commandsRendered(browser, beforeClear);
      await waitFor(browser, narrowed(ALL), "every branch and waiting review listed again");
      // Esc on sort:recent returns to the entries.
      await press(browser, ["Escape"], () => entriesFocused(browser), "back to the entries");
      // Ctrl+F focuses it too; Enter after a finished term returns to the entries.
      await press(browser, [primaryModifier, "f"], () => searchFocused(browser), "the search focused with Ctrl+F");
      await set("sort:recent branch:two ");
      await waitFor(browser, narrowed(["repo  scope-two"]), "branch:two narrowed to scope-two");
      await press(browser, ["Enter"], () => entriesFocused(browser), "Enter after a finished term returns to the entries");
      assert.equal(await query(), "sort:recent branch:two ", "leaving keeps the query");
    },
  ),

  // UX journey: J-PANE-SEARCH; UX edges: E-REVIEW-SEARCH-FOCUS E-PANE-SORT
  scenario<Record<string, never>>(
    "search-sort",
    "orders the Branches by their most recent Run with branches without Runs last, by project and branch with sort:name, and back",
    async () => {
      const { fixture } = await prepareSearch();
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "added", nativePath(fixture.root, "added"));
      return { fixture, data: {} };
    },
    async (browser) => {
      const { holds, names, set } = search(browser);
      // The Runs as dated: third's newest, then other's, the worktrees' and repo's.
      const recent = ["third  master", "other  master", "repo  scope-two", "repo  scope-one", "repo  master", "repo  added"];
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(recent), `the most recent Run first, without Runs last: ${JSON.stringify(await names())}`);
      await press(browser, ["/"], () => searchFocused(browser), "the search focused");
      await set("sort:name");
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(
        ["other  master", "repo  added", "repo  master", "repo  scope-one", "repo  scope-two", "third  master"]), "ordered by project and branch");
      await set("sort:recent");
      await waitFor(browser, holds("sort:recent"), "sort:recent typed back");
      await waitFor(browser, async () => JSON.stringify(await names()) === JSON.stringify(recent), "the most recent Run first again");
    },
  ),

  // UX journey: J-OPEN-ID; UX edge: E-REVIEW-READONLY
  scenario<{ id: string }>(
    "search-readonly",
    "opens a request by ID that is not a waiting review read-only, its facts under Debug, and Close returns to the Pane",
    async () => {
      const fixture = await create();
      const docs = requests(fixture).find((request) => request.evalId === "docs/lint");
      assert.ok(docs, "the docs eval's request was saved");
      return { fixture, data: { id: docs.id } };
    },
    async (browser, { data: { id } }) => {
      const record = () => browser.$('section.panel[aria-label="Main area"] [role="document"]');
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
      await fill(browser, '[aria-label="Request ID"]', id);
      await browser.$('[role="dialog"] button[type="submit"]').click();
      await waitFor(browser, async () => (await mainTitle(browser)) === "docs/lint · Docs build · read-only", "the request open read-only");
      await waitFor(browser, async () => (await record().getText()).includes("Build the docs."), "its Summary");
      const editable = await browser.execute(() => !!document.querySelector('section.panel[aria-label="Main area"] [role="document"]')
        ?.querySelector('input, textarea, select, [contenteditable="true"], [aria-label="Human review"]'));
      assert.equal(editable, false, "no review form or editing controls");
      const fields = () => browser.execute(() => Object.fromEntries([...document.querySelectorAll('section.panel[aria-label="Main area"] [role="document"] dt')]
        .map((key) => [key.textContent?.trim(), key.nextElementSibling?.textContent?.trim()])));
      assert.equal((await fields()).Request, undefined, "the Debug facts start folded");
      const debug = () => record().$("button*=Debug");
      await debug().click();
      await waitFor(browser, async () => (await debug().getAttribute("aria-expanded")) === "true" && (await fields()).Request === id, "Debug identifies the exact opened request");
      await browser.$('section.panel[aria-label="Main area"] > header').$("button*=Close").click();
      await waitFor(browser, () => entriesFocused(browser), "Close returned to the Pane");
    },
  ),

  // UX journeys: J-OPEN-ID J-LIVE; UX edges: E-REVIEW-READONLY E-MON-LIVE
  scenario<{ id: string }>(
    "search-readonly-live",
    "follows a request opened by ID read-only as it changes in another process, without opening it again",
    async () => {
      const fixture = await create();
      const ship = requests(fixture).find((request) => request.evalId === "ship/check");
      assert.ok(ship, "the ship eval's request was saved");
      return { fixture, data: { id: ship.id } };
    },
    async (browser, { fixture, data: { id } }) => {
      const status = () => browser.execute(() => document.querySelector('section.panel[aria-label="Main area"] [role="document"] .summary')?.textContent?.replace(/\s+/g, " ").trim() ?? "");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
      await fill(browser, '[aria-label="Request ID"]', id);
      await browser.$('[role="dialog"] button[type="submit"]').click();
      await waitFor(browser, async () => (await mainTitle(browser)) === "ship/check · Ship the release · read-only", "the waiting request open read-only");
      await waitFor(browser, async () => (await status()) !== "", "its status shown");
      assert.doesNotMatch(await status(), /GREEN/, `it waits for the release's sign-off: ${await status()}`);
      // The sign-off is given in another process: ship runs and passes, and the open request shows it.
      signOff(fixture, signoff(fixture).id);
      await waitFor(browser, async () => /GREEN/.test(await status()), `the open request followed its change: ${await status()}`, 30000);
      assert.equal(await mainTitle(browser), "ship/check · Ship the release · read-only", "the same request stays open");
    },
  ),
];
