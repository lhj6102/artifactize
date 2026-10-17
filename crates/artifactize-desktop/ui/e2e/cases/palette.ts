// The command palette in the real app: it opens by its button and by Ctrl+K, runs a command,
// and opens again after every way of closing it, screen switches included.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { verifyWaiting, addRepository, create, requests } from "../fixture.ts";
import { press, typeText } from "../keys.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { checkpoint, commandsRendered } from "../commands.ts";

import { scenario, type Scenario } from "../scenario.ts";
import { portablePath, primaryModifier, type NativePath } from "../platform.ts";
import { graphShown } from "../graph-view.ts";
import { clickAt } from "../pointer.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 10000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

type PaletteData = { originalId: string; followerId: string; otherId: string; otherRepo: NativePath };
type ListedRequest = { id: string; eval: string; status: string; claim: string; repository: string };

async function preparePalette() {
  const fixture = await create();
  const original = requests(fixture).find((request) => request.evalId === "release/signoff");
  assert.ok(original);
  // A second Run follows this identified original, not the other repository's sign-off.
  const followerRun = await verifyWaiting(fixture);
  // Another repository has the same eval IDs and distinct release notes.
  const otherRepo = await addRepository(fixture, "other");
  const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
  const follower = signoffs.find((request) => request.runId === followerRun);
  assert.ok(follower);
  const other = signoffs.find((request) => request.id !== original.id && request.id !== follower.id);
  assert.ok(other);
  return { fixture, args: ["--repo", fixture.repo], data: { originalId: original.id, followerId: follower.id, otherId: other.id, otherRepo } };
}

function waitingRequest(id: string, repository: NativePath): ListedRequest {
  return { id, eval: "? release/signoff", status: "WAITING_HUMAN", claim: "-", repository: portablePath(repository) };
}

async function listShown(browser: WebdriverIO.Browser, expected: ListedRequest[], selected: string) {
  // Both sides are ordered here by code point: the page's and Node's collations differ.
  const byId = (left: { id: string }, right: { id: string }) => left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  const wanted = [...expected].sort(byId);
  const snapshot = async () => {
    const shown = await browser.execute(() => ({
      rows: [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')].map((row) => {
        const cells = [...row.children].map((cell) => (cell as HTMLElement).innerText.trim());
        return { id: cells[1], eval: cells[0], status: cells[2], claim: cells[3], repository: cells[5] };
      }),
      selected: document.querySelector('[aria-label="Requests"] [aria-selected="true"] .mono')?.textContent?.trim(),
      error: document.querySelector(".review .crumbs .error")?.textContent?.trim() ?? null,
    }));
    return { ...shown, rows: shown.rows.sort(byId) };
  };
  try {
    await waitFor(browser, async () => {
      const shown = await snapshot();
      return shown.error === null && shown.selected === selected && isDeepStrictEqual(shown.rows, wanted);
    }, "the completed Review list has the exact saved requests and selection");
  } catch (error) {
    throw new Error(`Review list mismatch: ${JSON.stringify({ expected: { rows: wanted, selected, error: null }, actual: await snapshot() })}`, { cause: error });
  }
  assert.deepEqual(await snapshot(), { rows: wanted, selected, error: null });
}

function palette(browser: WebdriverIO.Browser) {
  const shown = () =>
    browser.execute(() => !!document.querySelector('[aria-label="Command palette"][data-state="open"]'));
  const opened = (how: string) => waitFor(browser, shown, `the palette opens (${how})`);
  const closed = (how: string) => waitFor(browser, async () => !(await shown()), `the palette closes (${how})`);
  const hidden = async () => !(await shown());
  /** Run the command labelled `label` from the open palette by typing and Enter. */
  const run = async (label: string) => {
    await choosePaletteCommand(browser, label);
    await press(browser, ["Enter"], hidden, `${label} selected and palette closed`);
  };
  // Reopening regression: run a command with a new visible outcome. An unchanged
  // list plus a completion counter cannot establish that Refresh actually reloaded it.
  const help = async (expected: ListedRequest[], selected: string) => {
    await run("Keyboard shortcuts");
    const dialog = () => browser.$('[role="dialog"]');
    await waitFor(browser, async () => await dialog().isDisplayed() &&
      (await dialog().getText()).includes("Keyboard shortcuts"), "the palette command opened keyboard help");
    await press(browser, ["Escape"], async () => !(await dialog().isExisting()), "the keyboard help closed");
    await listShown(browser, expected, selected);
  };
  /** Open the palette with Ctrl+K, or close it with Esc. */
  const ctrlK = (how: string) => press(browser, [primaryModifier, "k"], shown, `the palette opens (${how})`);
  const escape = (how: string) => press(browser, ["Escape"], hidden, `the palette closes (${how})`);
  /** Open a request by ID with Ctrl+O; its Human review shows. */
  const openId = async (id: string, canonical = id) => {
    await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
    await browser.$('[aria-label="Request ID"]').click();
    await typeText(browser, id);
    const before = await checkpoint(browser);
    const completed = before.activity.commands.reviewDetail?.completed ?? 0;
    await browser.$('[role="dialog"] button[type="submit"]').click();
    // Opening an already shown original/follower can leave the same review visible.
    // Correlate this ID lookup's completion before accepting the rendered review.
    await waitFor(browser, async () => ((await checkpoint(browser)).activity.commands.reviewDetail?.completed ?? 0) === completed + 1,
      `the lookup of ${id} completed`);
    await commandsRendered(browser, before);
    await waitFor(browser, async () =>
      !(await browser.$('[aria-label="Request ID"]').isExisting()) && await browser.$('[aria-label="Human review"]').isExisting(),
    `the request ID dialog resolves to the review of ${id}`);
    assert.equal(await browser.$(".review .crumbs .error").isExisting(), false, "the ID lookup succeeded");
    await browser.$('[aria-label="Human review"] header').waitForDisplayed({ timeout: 15000 });
    const technical = browser.$('[aria-label="Human review"]').$("button*=Technical");
    const expanded = await technical.getAttribute("aria-expanded") === "true";
    if (!expanded) await technical.click();
    await waitFor(browser, () => browser.execute((expected) => {
      const key = [...document.querySelectorAll('[aria-label="Human review"] dt')].find((field) => field.textContent === "Request");
      return key?.nextElementSibling?.textContent === expected;
    }, canonical), `the rendered canonical request is ${canonical}`);
    if (!expanded) await technical.click();
  };
  return { shown, opened, closed, hidden, run, help, ctrlK, escape, openId };
}

export const scenarios: Scenario[] = [
  // UX edges: E-PALETTE-OPEN E-PALETTE-CLOSE
  scenario<PaletteData>(
    "palette-reopen",
    "opens by its button and by Ctrl+K, and again after Esc, several times",
    async () => preparePalette(),
    async (browser) => {
      const { opened, ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      for (let round = 0; round < 3; round += 1) {
        await browser.$("button*=Commands").click();
        await opened("button");
        await escape("Esc");
        await ctrlK("Ctrl+K");
        await escape("Esc");
      }
    },
  ),
  // UX edges: E-PALETTE-OPEN E-PALETTE-FILTER
  scenario<PaletteData>(
    "palette-filter",
    "lists the enabled Monitor commands and filters them by what is typed",
    async () => preparePalette(),
    async (browser) => {
      const { ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      await graphShown(browser, "a:release");
      const listed = () => browser.execute(() => [...document.querySelectorAll("[data-command-item]")]
        .map((item) => item.querySelector("span")?.textContent?.trim() ?? "").sort());
      await ctrlK("filter");
      // An unopened Monitor Detail offers Monitor navigation, not Review or Human actions.
      // These are the person's commands for this screen, not a registry-derived oracle.
      const expected = [
        "Command palette", "Keyboard shortcuts", "Show Monitor", "Show Review", "Show Settings", "Quit",
        "Refresh", "Next attention", "Focus Scope", "Focus Runs", "Focus Run graph",
        "Open Detail of the selected node", "Open the Run's Detail",
      ].sort();
      await waitFor(browser, async () => isDeepStrictEqual(await listed(), expected), "the enabled Monitor commands are rendered");
      assert.deepEqual(await listed(), expected, "all and only the enabled commands of the Monitor");
      await typeText(browser, "Show Review");
      await waitFor(browser, async () => JSON.stringify(await listed()) === JSON.stringify(["Show Review"]), "only the matching command remains");
      assert.deepEqual(await listed(), ["Show Review"]);
      await escape("Esc");
    },
  ),
  // UX edges: E-PALETTE-RUN E-SCREEN-MONITOR E-SCREEN-REVIEW E-SCREEN-SETTINGS
  scenario<PaletteData>(
    "palette-screens",
    "runs commands that switch screens and opens again after each",
    async () => preparePalette(),
    async (browser) => {
      const { opened, closed, run, ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      for (const label of ["Show Review", "Show Monitor", "Show Review", "Show Monitor"]) {
        await ctrlK(`before ${label}`);
        await run(label);
        await closed(label);
        const screen = label === "Show Review" ? "Review" : "Monitor";
        await waitFor(browser,
          () => browser.execute((name) => document.querySelector('[role="tab"][data-state="active"]')?.textContent === name, screen),
          `${screen} shows`,
        );
        await browser.$("button*=Commands").click();
        await opened(`after ${label}`);
        await escape("Esc");
      }
    },
  ),
  scenario<PaletteData>(
    "palette-id-follower",
    "opens after requests were opened by ID on the review screen, a follower included",
    async () => preparePalette(),
    async (browser, { fixture, data }) => {
      const { opened, help, ctrlK, escape, openId } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
      const source = (request: (typeof signoffs)[number]) =>
        (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId;
      const original = signoffs.find((request) => request.id === data.originalId);
      const follower = signoffs.find((request) => request.id === data.followerId);
      assert.ok(original && follower, "an original and its follower");
      assert.equal(source(original), original.id, "the prepared original owns its execution");
      assert.equal(source(follower), original.id, "the identified follower shares this original's execution");
      const expected = [waitingRequest(original.id, fixture.repo)];
      await press(
        browser,
        [primaryModifier, "2"],
        () => browser.execute(() => document.querySelector('[role="tab"][data-state="active"]')?.textContent === "Review"),
        "the Review screen",
      );
      await listShown(browser, expected, original.id);
      for (const id of [original.id, follower.id, original.id]) {
        await openId(id, original.id);
        await browser.$("button*=Commands").click();
        await opened(`button after opening ${id}`);
        await help(expected, original.id);
        await ctrlK(`Ctrl+K after opening ${id}`);
        await escape("Esc");
      }
    },
  ),
  scenario<PaletteData>(
    "palette-search",
    "opens after a request opened by ID, Esc and a search of every repository",
    async () => preparePalette(),
    async (browser, { fixture, data }) => {
      const { opened, closed, run, help, ctrlK, openId } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      await press(browser, [primaryModifier, "2"], () => browser.execute(() =>
        document.querySelector('[role="tab"][data-state="active"]')?.textContent === "Review"), "this case's Review screen");
      const original = requests(fixture).find((request) => request.id === data.originalId);
      assert.ok(original);
      const all = [waitingRequest(data.otherId, data.otherRepo), waitingRequest(original.id, fixture.repo)];
      const scoped = [waitingRequest(original.id, fixture.repo)];
      const listed = () =>
        browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
      for (let round = 0; round < 2; round += 1) {
        await openId(original.id);
        // Esc reaches the review only once focus is inside it.
        await waitFor(browser,
          () => browser.execute(() => !!document.activeElement?.closest('[aria-label="Human review"]')),
          "focus in the review",
        );
        await press(browser, ["Escape"], async () => !(await browser.$('[aria-label="Human review"]').isExisting()), "back on the list");
        // The launch searches its own repository; a search without the repo: term finds both.
        const focused = (label: string) => () => browser.execute((name) => document.activeElement?.getAttribute("aria-label") === name, label);
        await press(browser, ["/"], focused("Search requests"), "focus in the search");
        await press(
          browser,
          ["Escape"],
          () => browser.execute(() => (document.activeElement as HTMLInputElement).value === ""),
          "the search cleared",
        );
        // A space ends the term and closes the suggestions, so Enter leaves the field.
        const beforeSearch = await checkpoint(browser);
        await typeText(browser, "kind:human status:waiting_human ");
        await commandsRendered(browser, beforeSearch);
        await waitFor(browser, async () => (await listed()) === 2, "the sign-offs of both repositories");
        await press(browser, ["Enter"], focused("Requests"), "focus on the list");
        const selected = await browser.$('[aria-label="Requests"] [aria-selected="true"] .mono').getText();
        assert.ok(all.some((request) => request.id === selected), "one of the exact prepared requests is selected before opening help");
        await listShown(browser, all, selected);
        await browser.$("button*=Commands").click();
        await opened(`button after the search, round ${round}`);
        await help(all, selected);
        await ctrlK(`Ctrl+K after the search, round ${round}`);
        await run("Show waiting Human reviews");
        await closed("the search reset");
        await waitFor(browser, async () => (await listed()) === 1, "this repository's sign-off again");
        await listShown(browser, scoped, original.id);
      }
    },
  ),
  scenario<PaletteData>(
    "palette-outside",
    "opens again after a click outside closed it",
    async () => preparePalette(),
    async (browser) => {
      const { closed, ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      // Errors the page throws (a broken palette threw while rendering) fail the scenario.
      await browser.execute(() => {
        const errors: string[] = [];
        (window as unknown as { errors: string[] }).errors = errors;
        window.addEventListener("error", (event) => errors.push(event.message));
      });
      await ctrlK("Ctrl+K");
      await clickAt(browser, { x: 20, y: 600 });
      await closed("outside click");
      await ctrlK("after the outside click");
      await escape("Esc");
      assert.deepEqual(await browser.execute(() => (window as unknown as { errors: string[] }).errors), []);
    },
  ),
];
