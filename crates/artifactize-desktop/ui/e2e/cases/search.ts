// The review screen's request search on real state: several repositories share eval IDs. The
// launch lists its own repository's waiting sign-off; a search reaches the others, completion
// takes a typed partial key and its value, and any other request opens read-only.
import assert from "node:assert/strict";
import { addRepository, artifactize, create, requests } from "../fixture.ts";
import { press, typeText } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";

import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

async function prepareSearch() {
  const fixture = await create();
  await addRepository(fixture, "other");
  await addRepository(fixture, "third");
  return { fixture, args: ["review", "--repo", fixture.repo], data: {} };
}

function search(browser: WebdriverIO.Browser) {
  /** Read the rendered grid rows independently of provider-specific element-text behavior. */
  const rows = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')].map((row) => (row as HTMLElement).innerText),
    );
  const query = () => browser.execute(() => (document.querySelector('[aria-label="Search requests"]') as HTMLInputElement).value);
  const type = (text: string) => typeText(browser, text);
  const focused = (label: string) => () => browser.execute((name) => document.activeElement?.getAttribute("aria-label") === name, label);
  const holds = (text: string) => async () => (await query()) === text;
  /** Only an opened RecordDetail has a document; a list row or Preview cannot match. */
  const record = () => browser.$('[role="document"]');
  const detail = () => browser.execute(() => {
    const opened = document.querySelector<HTMLElement>('[role="document"]');
    const pane = opened?.closest("section.panel");
    if (!opened || !pane) return null;
    return {
      title: pane.querySelector("header h2")?.textContent?.trim(),
      fields: Object.fromEntries([...opened.querySelectorAll("dt")].map((key) => [key.textContent?.trim(), key.nextElementSibling?.textContent?.trim()])),
      editable: !!opened.querySelector('input, textarea, select, [contenteditable="true"], [aria-label="Human review"]'),
      close: !!pane.querySelector('button[title="Back to the list"]'),
    };
  });
  const suggestions = () =>
    browser.execute(() => [...document.querySelectorAll('[aria-label="Suggestions"] [role="option"]')].map((option) => option.textContent?.trim()));
  return { rows, query, type, focused, holds, record, detail, suggestions };
}

export const scenarios: Scenario[] = [
  // UX edges: E-LAUNCH-REVIEW
  scenario<Record<string, never>>(
    "search-scoped",
    "starts on the named repository's waiting sign-off",
    async () => prepareSearch(),
    async (browser) => {
      const { rows, query } = search(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await waitFor(browser, async () => (await rows()).length === 1, "one waiting row");
      assert.match((await rows())[0], /release\/signoff/);
      assert.match(await query(), /^kind:human status:waiting_human repo:/);
      await commandsRendered(browser);
      const activity = (await checkpoint(browser)).activity;
      assert.ok(activity.commands.reviewSearch?.started > 0, "the context-captured search dispatch is observed");
      assert.ok(activity.commands.reviewSearch?.completed > 0, "normal search completion is observed");
      assert.equal(activity.search?.query, await query());
      assert.equal(activity.search?.completed, true);
    },
  ),
  // J-REVIEW-SEARCH; UX edges: E-SCREEN-MONITOR E-SCREEN-REVIEW E-REVIEW-SEARCH-FOCUS E-REVIEW-SEARCH-TERMS
  scenario<Record<string, never>>(
    "search-empty-focus",
    "focuses search with / after switching to Review with no waiting requests, without taking typed slashes",
    async () => prepareSearch(),
    async (browser, { fixture }) => {
      const { rows, query, type, focused, holds } = search(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await waitFor(browser, async () => (await rows()).length === 1, "the repository's waiting sign-off");
      // The public request queue spans every repository in this state. Complete the exact
      // row shown by this repository-scoped search, not its newest same-eval request elsewhere.
      const id = await browser.execute(() => document.querySelector('[aria-label="Requests"] [role="option"] .mono')?.textContent?.trim());
      assert.ok(id, "the visible waiting row names its request");
      const waiting = requests(fixture).find((request) => request.id === id);
      assert.ok(waiting, "the displayed request is in the public queue");
      assert.equal(waiting.evalId, "release/signoff", "the displayed request is the sign-off");
      assert.equal(waiting.status, "WAITING_HUMAN", "that exact displayed request still waits");
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "alice"), 0);
      assert.equal(artifactize(fixture, "request", "submit", id, "--reviewer", "alice", "--verdict", "GREEN", "--fields", '{"approved":true,"comment":"ok"}'), 0);
      assert.equal(requests(fixture).find((request) => request.id === id)?.status, "GREEN", "the exact displayed request is settled in the public queue");
      await waitFor(browser, async () => (await rows()).length === 0 &&
        (await browser.execute(() => document.body.innerText)).includes("No Human reviews are waiting."), "the settled, empty waiting list");
      assert.equal(await browser.$('[aria-label="Requests"]').isExisting(), false, "there is no Requests list to receive keyboard focus");
      const kept = await query();
      await press(browser, [primaryModifier, "1"], focused("Runs"), "Monitor focused");
      await press(browser, ["/"], async () => await focused("Runs")() && (await query()) === kept, "inactive Review leaves focus and its query alone");
      await press(browser, [primaryModifier, "2"], async () =>
        (await browser.$('[role="tab"][aria-selected="true"]').getText()) === "Review", "the empty Review screen shown");
      await press(browser, ["/"], focused("Search requests"), "search focused without a Requests list");
      assert.equal(await query(), kept, "the shortcut focuses search without typing a slash");
      await press(browser, ["Escape"], holds(""), "the waiting query cleared for typing");
      await type("release/signoff ");
      assert.equal(await query(), "release/signoff ", "a slash typed in the field remains part of the query");
      await waitFor(browser, async () => (await rows()).length === 3 &&
        (await rows()).every((row) => row.includes("release/signoff")), "the typed query searches the saved sign-offs");
    },
  ),
  // J-REVIEW-SEARCH; UX edges: E-REVIEW-SEARCH-FOCUS E-REVIEW-SEARCH-TERMS E-REVIEW-SUGGEST E-REVIEW-SUGGEST-CLOSE E-REVIEW-SEARCH-CLEAR E-REVIEW-SEARCH-LEAVE
  scenario<Record<string, never>>(
    "search-suggestions",
    "filters every repository's requests and takes suggestions for a partial key and value",
    async () => prepareSearch(),
    async (browser) => {
      const { rows, query, type, focused, holds, suggestions } = search(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await press(browser, ["/"], focused("Search requests"), "focus in the search");
      // Esc clears the query: every request of every repository.
      await press(browser, ["Escape"], holds(""), "the query cleared");
      await waitFor(browser, async () => (await rows()).length === 3 * 3, "every request of the three repositories");

      await type("kind:hu");
      await waitFor(browser, async () => (await suggestions()).includes("kind:human"), "the kind suggestion");
      // Esc closes the suggestions and keeps the query; the next Esc clears it.
      await press(browser, ["Escape"], async () => (await suggestions()).length === 0, "the suggestions closed");
      assert.equal(await query(), "kind:hu", "the query is kept");
      await press(browser, ["Escape"], holds(""), "the query cleared");
      await type("kind:hu");
      await waitFor(browser, async () => (await suggestions()).includes("kind:human"), "the kind suggestion again");
      await press(browser, ["Tab"], holds("kind:human "), "the kind taken");
      await waitFor(browser, async () => (await rows()).length === 3, "the three sign-offs");

      await type("rep");
      await waitFor(browser, async () => (await suggestions()).includes("repo:"), "the repo key");
      const chosen = () =>
        browser.execute(() => document.querySelector('[aria-label="Suggestions"] [aria-selected="true"]')?.textContent?.trim() ?? null);
      await press(browser, ["ArrowDown"], async () => (await chosen()) === "repo:", "the repo key chosen", { handled: true });
      await press(browser, ["Enter"], holds("kind:human repo:"), "the repo key taken");
      // A taken key offers the listed repositories' folders.
      await waitFor(browser, async () => (await suggestions()).includes("repo:other"), "the repository values");
      await type("oth");
      await waitFor(browser, async () => (await suggestions()).includes("repo:other"), "the other repository");
      await press(browser, ["Tab"], holds("kind:human repo:other "), "the repository taken");
      await waitFor(browser, async () => {
        const shown = await rows();
        return shown.length === 1 && /release\/signoff/.test(shown[0]) && /other/.test(shown[0]);
      }, "the other repository's sign-off");

      // Continue this same journey after accepting the completion: Esc first closes
      // another offered suggestion without losing that query, then clears all terms,
      // restores the complete request list, and finally returns focus to Requests.
      await type("sta");
      await waitFor(browser, async () => (await suggestions()).length > 0, "suggestions after the accepted repository");
      const kept = await query();
      await press(browser, ["Escape"], async () => (await suggestions()).length === 0, "the post-completion suggestions closed");
      assert.equal(await query(), kept, "closing suggestions preserves the completed terms and partial input");
      const beforeClear = await checkpoint(browser);
      await press(browser, ["Escape"], holds(""), "the same journey's query cleared");
      await commandsRendered(browser, beforeClear);
      await waitFor(browser, async () => (await rows()).length === 3 * 3, "all three repositories' requests restored after clearing");
      await press(browser, ["Escape"], focused("Requests"), "the empty search returns focus to Requests");
    },
  ),
  // UX edges: E-REVIEW-READONLY E-REVIEW-SEARCH-LEAVE
  scenario<Record<string, never>>(
    "search-readonly",
    "opens a request that is not a Human review read-only",
    async () => prepareSearch(),
    async (browser) => {
      const { rows, type, focused, holds, record, detail } = search(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await press(browser, ["/"], focused("Search requests"), "this case's search field");
      // Esc clears the query; a new search, then Enter leaves for the list.
      await press(browser, ["Escape"], holds(""), "the query cleared");
      // A space ends the term and closes the suggestions, so Enter leaves the field.
      await type("kind:runtime eval:docs repo:third ");
      await press(browser, ["Enter"], focused("Requests"), "focus on the list");
      // An empty query's Esc leaves the field for the list as well.
      await press(browser, ["/"], focused("Search requests"), "focus in the search again");
      await press(browser, ["Escape"], holds(""), "the query cleared again");
      await press(browser, ["Escape"], focused("Requests"), "Esc left for the list");
      await press(browser, ["/"], focused("Search requests"), "focus in the search a third time");
      // The same query was searched before: only a newer, completed and rendered search of
      // it, not the rows the earlier one left, shows the current result.
      const beforeFinal = await checkpoint(browser);
      await type("kind:runtime eval:docs repo:third ");
      await press(browser, ["Enter"], focused("Requests"), "focus on the list again");
      await commandsRendered(browser, beforeFinal);
      await waitFor(browser, async () => {
        const shown = await rows();
        return shown.length === 1 && shown[0].includes("docs/lint") && shown[0].includes("third");
      }, "the third repository's docs eval");
      const listed = (await rows())[0];
      const requestId = /run-\S+/.exec(listed)?.[0];
      assert.ok(requestId, `the row names its request: ${listed}`);
      assert.equal(await record().isExisting(), false, "the read-only Preview is not an opened Detail");
      await press(browser, ["Enter"], async () => {
        const opened = await detail();
        return opened?.title === "docs/lint · Docs build · read-only" && opened.close;
      }, "the non-Human read-only Detail opened");
      await waitFor(browser, async () => (await detail())?.fields.Request === requestId, "the default Detail identifies the exact opened request without Technical");
      const opened = await detail();
      assert.ok(opened);
      assert.equal(opened.fields.Request, requestId, "this opened Detail belongs to the exact listed request by default");
      assert.equal(opened.title, "docs/lint · Docs build · read-only", "this Detail names the eval and its read-only state");
      assert.equal(opened.editable, false, "this non-Human Detail has no review form or editing controls");
      assert.ok(opened.close, "the opened Detail offers Back to the list");
      assert.equal(opened.fields.Fingerprint, undefined, "additional technical fields start folded");
      const technical = () => record().$("button*=Technical");
      await technical().click();
      await waitFor(browser, async () => (await technical().getAttribute("aria-expanded")) === "true" && !!(await detail())?.fields.Fingerprint, "the opened Detail renders its additional technical record");
      assert.equal((await detail())?.fields.Request, requestId, "Technical preserves the opened request's identity");
      await press(browser, ["Escape"], async () => !(await record().isExisting()) && await focused("Requests")(), "the opened Detail closed back to the list");
    },
  ),
];
