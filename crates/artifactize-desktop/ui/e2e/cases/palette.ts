// The command palette in the real app: it opens by its button and by Ctrl+K, filters and runs a
// command, and opens again after every way of closing it, after reviews opened by ID (a
// follower included) and after a search of the Pane.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { verifyWaiting, addRepository, create, requests } from "../fixture.ts";
import { fill, press, typeText } from "../keys.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";
import { detail, graphShown } from "../graph-view.ts";
import { clickAt } from "../pointer.ts";
import { entriesFocused, mainTitle, SEARCH, searchFocused, waiting } from "../pane.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 10000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

type PaletteData = { originalId: string; followerId: string; otherId: string };

async function preparePalette() {
  const fixture = await create();
  const original = requests(fixture).find((request) => request.evalId === "release/signoff");
  assert.ok(original);
  // A second Run follows this identified original, not the other repository's sign-off.
  const followerRun = await verifyWaiting(fixture);
  // Another repository has the same eval IDs and distinct release notes.
  await addRepository(fixture, "other");
  const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
  const follower = signoffs.find((request) => request.runId === followerRun);
  assert.ok(follower);
  const other = signoffs.find((request) => request.id !== original.id && request.id !== follower.id);
  assert.ok(other);
  return { fixture, args: ["--repo", fixture.repo], data: { originalId: original.id, followerId: follower.id, otherId: other.id } };
}

/** The Waiting stack holds exactly these reviews, in any order. */
async function waitingShown(browser: WebdriverIO.Browser, ids: string[]) {
  const shown = async () => (await waiting(browser)).rows.map((row) => row.id).sort();
  await waitFor(browser, async () => isDeepStrictEqual(await shown(), [...ids].sort()), `the Waiting stack holds ${ids.join(", ")}: ${JSON.stringify(await shown())}`);
}

/** Errors the page throws (a broken palette threw while rendering) fail the scenario. */
function watchErrors(browser: WebdriverIO.Browser) {
  return browser.execute(() => {
    const errors: string[] = [];
    (window as unknown as { errors: string[] }).errors = errors;
    window.addEventListener("error", (event) => errors.push(event.message));
  });
}
const errors = (browser: WebdriverIO.Browser) => browser.execute(() => (window as unknown as { errors: string[] }).errors);

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
  /** A command with a new visible outcome: the keys open, and close again. */
  const help = async () => {
    await run("Keyboard shortcuts");
    const dialog = () => browser.$('[role="dialog"]');
    await waitFor(browser, async () => await dialog().isDisplayed() &&
      (await dialog().getText()).includes("Keyboard shortcuts"), "the palette command opened keyboard help");
    await press(browser, ["Escape"], async () => !(await dialog().isExisting()), "the keyboard help closed");
  };
  /** Open the palette with Ctrl+K, or close it with Esc. */
  const ctrlK = (how: string) => press(browser, [primaryModifier, "k"], shown, `the palette opens (${how})`);
  const escape = (how: string) => press(browser, ["Escape"], hidden, `the palette closes (${how})`);
  /** Open a request by ID with Ctrl+O; the review of `canonical` shows. */
  const openId = async (id: string, canonical = id) => {
    await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
    await browser.$('[aria-label="Request ID"]').click();
    await typeText(browser, id);
    const before = await checkpoint(browser);
    const completed = before.activity.commands.requestDetail?.completed ?? 0;
    await browser.$('[role="dialog"] button[type="submit"]').click();
    // Opening an already shown original/follower can leave the same review visible.
    // Correlate this ID lookup's completion before accepting the rendered review.
    await waitFor(browser, async () => ((await checkpoint(browser)).activity.commands.requestDetail?.completed ?? 0) === completed + 1,
      `the lookup of ${id} completed`);
    await commandsRendered(browser, before);
    await waitFor(browser, async () =>
      !(await browser.$('[aria-label="Request ID"]').isExisting()) && (await mainTitle(browser)) === "Review" && await browser.$('[aria-label="Human review"]').isExisting(),
    `the request ID dialog resolves to the review of ${id}`);
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
  // UX journey: J-PALETTE; UX edges: E-PALETTE-OPEN E-PALETTE-CLOSE
  scenario<PaletteData>(
    "palette-reopen",
    "opens by its button and by Ctrl+K, and again after Esc, several times",
    async () => preparePalette(),
    async (browser) => {
      const { opened, ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      await watchErrors(browser);
      for (let round = 0; round < 3; round += 1) {
        await browser.$("button*=Commands").click();
        await opened("button");
        await escape("Esc");
        await ctrlK("Ctrl+K");
        await escape("Esc");
      }
      assert.deepEqual(await errors(browser), []);
    },
  ),
  // UX journey: J-PALETTE; UX edges: E-PALETTE-OPEN E-PALETTE-FILTER
  scenario<PaletteData>(
    "palette-filter",
    "lists the enabled commands of the shown Run and filters them by what is typed",
    async () => preparePalette(),
    async (browser) => {
      const { ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      await graphShown(browser, "a:release");
      const listed = () => browser.execute(() => [...document.querySelectorAll("[data-command-item]")]
        .map((item) => item.querySelector("span")?.textContent?.trim() ?? "").sort());
      await ctrlK("filter");
      // With no Detail open, the window's, the Pane's and the Main area's commands, not a
      // Detail's or a Human review's. These are the person's commands, not a registry oracle.
      const expected = [
        "Command palette", "Keyboard shortcuts", "Show Settings", "Refresh", "Quit",
        "Search branches and reviews", "Focus the Pane", "Focus the Main area", "Open a request by ID", "Open the Run's Detail",
      ].sort();
      await waitFor(browser, async () => isDeepStrictEqual(await listed(), expected), `the enabled commands are rendered: ${JSON.stringify(await listed())}`);
      assert.deepEqual(await listed(), expected, "all and only the enabled commands");
      await typeText(browser, "Show Set");
      await waitFor(browser, async () => JSON.stringify(await listed()) === JSON.stringify(["Show Settings"]), "only the matching command remains");
      await escape("Esc");
    },
  ),
  // UX journey: J-PALETTE; UX edges: E-PALETTE-RUN E-SCREEN-SETTINGS
  scenario<PaletteData>(
    "palette-run",
    "runs commands to their targets, opening again after each",
    async () => preparePalette(),
    async (browser, { data }) => {
      const { opened, closed, run, ctrlK, escape } = palette(browser);
      await graphShown(browser, "a:release");
      const before = await mainTitle(browser);
      const steps: [string, () => Promise<boolean>][] = [
        ["Show Settings", async () => (await mainTitle(browser)) === "Settings"],
        ["Open the Run's Detail", async () => (await detail(browser))?.title === before],
        ["Focus the Pane", () => entriesFocused(browser)],
        ["Search branches and reviews", () => searchFocused(browser)],
      ];
      for (const [label, outcome] of steps) {
        await ctrlK(`before ${label}`);
        await run(label);
        await closed(label);
        await waitFor(browser, outcome, `${label} reached its target`);
        await browser.$("button*=Commands").click();
        await opened(`after ${label}`);
        await escape("Esc");
        // Esc, not the palette, leaves Settings for the view before it.
        if (label === "Show Settings") await press(browser, ["Escape"], async () => (await mainTitle(browser)) === before, "the Run again");
      }
      // Open a request by ID: its dialog, then the review.
      await ctrlK("before Open a request by ID");
      await run("Open a request by ID");
      await waitFor(browser, () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
      await fill(browser, '[aria-label="Request ID"]', data.otherId);
      await browser.$('[role="dialog"] button[type="submit"]').click();
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "the review opened by ID");
      await ctrlK("after the review opened");
      await escape("Esc");
    },
  ),
  // UX journeys: J-PALETTE J-OPEN-ID; UX edges: E-PALETTE-OPEN E-REVIEW-OPEN-ID
  scenario<PaletteData>(
    "palette-id-follower",
    "opens after requests were opened by ID, a follower included",
    async () => preparePalette(),
    async (browser, { fixture, data }) => {
      const { opened, help, ctrlK, escape, openId } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
      const source = (request: (typeof signoffs)[number]) =>
        (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId;
      const original = signoffs.find((request) => request.id === data.originalId);
      const follower = signoffs.find((request) => request.id === data.followerId);
      assert.ok(original && follower, "an original and its follower");
      assert.equal(source(original), original.id, "the prepared original owns its execution");
      assert.equal(source(follower), original.id, "the identified follower shares this original's execution");
      // A follower waits as its original: one row for both, one for the other repository's.
      await waitingShown(browser, [original.id, data.otherId]);
      for (const id of [original.id, follower.id, original.id]) {
        await openId(id, original.id);
        await browser.$("button*=Commands").click();
        await opened(`button after opening ${id}`);
        await help();
        await waitingShown(browser, [original.id, data.otherId]);
        await ctrlK(`Ctrl+K after opening ${id}`);
        await escape("Esc");
      }
    },
  ),
  // UX journey: J-PALETTE; UX edge: E-PALETTE-OPEN
  scenario<PaletteData>(
    "palette-search",
    "opens after a review opened by ID, Esc and a search of the Pane",
    async () => preparePalette(),
    async (browser, { data }) => {
      const { opened, help, ctrlK, escape, openId } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      for (let round = 0; round < 2; round += 1) {
        await openId(data.originalId);
        // Esc reaches the review only once focus is inside it.
        await waitFor(browser, () => browser.execute(() => !!document.activeElement?.closest('[aria-label="Human review"]')), "focus in the review");
        await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane");
        await press(browser, ["/"], () => searchFocused(browser), "focus in the search");
        const before = await checkpoint(browser);
        await fill(browser, SEARCH, "sort:recent other ");
        await commandsRendered(browser, before);
        await waitingShown(browser, [data.otherId]);
        await press(browser, ["Enter"], () => entriesFocused(browser), "back on the entries");
        await browser.$("button*=Commands").click();
        await opened(`button after the search, round ${round}`);
        await help();
        await ctrlK(`Ctrl+K after the search, round ${round}`);
        await escape("Esc");
        // The search back to sort:recent: every waiting review again.
        await press(browser, ["/"], () => searchFocused(browser), "focus in the search again");
        await press(browser, ["Escape"], async () => (await browser.$(SEARCH).getValue()) === "sort:recent", "sort:recent put back");
        await waitingShown(browser, [data.originalId, data.otherId]);
        await press(browser, ["Escape"], () => entriesFocused(browser), "back on the entries");
      }
    },
  ),
  // UX journey: J-PALETTE; UX edge: E-PALETTE-CLOSE
  scenario<PaletteData>(
    "palette-outside",
    "opens again after a click outside closed it",
    async () => preparePalette(),
    async (browser) => {
      const { closed, ctrlK, escape } = palette(browser);
      await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
      await watchErrors(browser);
      await ctrlK("Ctrl+K");
      await clickAt(browser, { x: 20, y: 600 });
      await closed("outside click");
      await ctrlK("after the outside click");
      await escape("Esc");
      assert.deepEqual(await errors(browser), []);
    },
  ),
];
