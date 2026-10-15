// The command palette in the real app: it opens by its button and by Ctrl+K, runs a command,
// and opens again after every way of closing it, screen switches included.
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { verifyWaiting, addRepository, create, type Fixture, requests } from "./fixture.ts";
import { checkBuild, type Session, start } from "./driver.ts";
import { press, typeText } from "./keys.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 50));
  }
  assert.fail(`timed out: ${what}`);
}

describe("command palette", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;
  let fixture: Fixture;
  const shown = () =>
    browser.execute(() => !!document.querySelector('[aria-label="Command palette"][data-state="open"]'));
  const opened = (how: string) => eventually(shown, `the palette opens (${how})`);
  const closed = (how: string) => eventually(async () => !(await shown()), `the palette closes (${how})`);
  const hidden = async () => !(await shown());
  /** Run the command labelled `label` from the open palette by typing and Enter. */
  const run = async (label: string) => {
    await typeText(browser, label);
    await press(browser, ["Enter"], hidden, `${label} ran`);
  };
  /** Open the palette with Ctrl+K, or close it with Esc. */
  const ctrlK = (how: string) => press(browser, ["Control", "k"], shown, `the palette opens (${how})`);
  const escape = (how: string) => press(browser, ["Escape"], hidden, `the palette closes (${how})`);
  /** Open a request by ID with Ctrl+O; its Human review shows. */
  const openId = async (id: string) => {
    await press(browser, ["Control", "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
    await browser.$('[aria-label="Request ID"]').click();
    await typeText(browser, id);
    await press(browser, ["Enter"], async () =>
      !(await browser.$('[aria-label="Request ID"]').isExisting()) && await browser.$('[aria-label="Human review"]').isExisting(),
    `the request ID dialog resolves to the review of ${id}`);
  };

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    // A second Run joins the waiting sign-off: its request is a follower of the first one.
    await verifyWaiting(fixture);
    // Another repository with the same eval IDs, which the review search reaches.
    await addRepository(fixture, "other");
    session = await start(fixture, "--repo", fixture.repo);
    browser = session.browser;
    await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
    // Errors the page throws (a broken palette threw while rendering) fail the suite.
    await browser.execute(() => {
      const errors: string[] = [];
      (window as unknown as { errors: string[] }).errors = errors;
      window.addEventListener("error", (event) => errors.push(event.message));
    });
  });

  afterEach(async () => {
    await session?.dispose();
  });

  // UX edges: E-PALETTE-OPEN E-PALETTE-CLOSE
  it("opens by its button and by Ctrl+K, and again after Esc, several times", async () => {
    for (let round = 0; round < 3; round += 1) {
      await browser.$("button*=Commands").click();
      await opened("button");
      await escape("Esc");
      await ctrlK("Ctrl+K");
      await escape("Esc");
    }
  });

  // UX edges: E-PALETTE-OPEN E-PALETTE-FILTER
  it("lists the enabled Monitor commands and filters them by what is typed", async () => {
    await browser.$('[role="treeitem"][aria-selected="true"]').waitForExist({ timeout: 15000 });
    const listed = () => browser.execute(() => [...document.querySelectorAll("[data-command-item]")]
      .map((item) => item.querySelector("span")?.textContent?.trim() ?? "").sort());
    await ctrlK("filter");
    // An unopened Monitor Detail offers Monitor navigation, not Review or Human actions.
    // These are the person's commands for this screen, not a registry-derived oracle.
    assert.deepEqual(await listed(), [
      "Command palette", "Keyboard shortcuts", "Show Monitor", "Show Review", "Show Settings", "Quit",
      "Refresh", "Next attention", "Focus Scope", "Focus Runs", "Focus Run tree",
      "Show the Run graph", "Open Detail of the selected row",
    ].sort(), "all and only the enabled commands of the Monitor");
    await typeText(browser, "Show Review");
    await eventually(async () => JSON.stringify(await listed()) === JSON.stringify(["Show Review"]), "only the matching command remains");
    assert.deepEqual(await listed(), ["Show Review"]);
    await escape("Esc");
  });

  // UX edges: E-PALETTE-RUN E-SCREEN-MONITOR E-SCREEN-REVIEW E-SCREEN-SETTINGS
  it("runs commands that switch screens and opens again after each", async () => {
    for (const label of ["Show Review", "Show Monitor", "Show Review", "Show Monitor"]) {
      await ctrlK(`before ${label}`);
      await run(label);
      await closed(label);
      const screen = label === "Show Review" ? "Review" : "Monitor";
      await eventually(
        () => browser.execute((name) => document.querySelector('[role="tab"][data-state="active"]')?.textContent === name, screen),
        `${screen} shows`,
      );
      await browser.$("button*=Commands").click();
      await opened(`after ${label}`);
      await escape("Esc");
    }
  });

  it("opens after requests were opened by ID on the review screen, a follower included", async () => {
    const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
    const source = (request: (typeof signoffs)[number]) =>
      (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId;
    const original = signoffs.find((request) => source(request) === request.id);
    const follower = signoffs.find((request) => source(request) !== request.id);
    assert.ok(original && follower, "an original and its follower");
    await press(
      browser,
      ["Control", "2"],
      () => browser.execute(() => document.querySelector('[role="tab"][data-state="active"]')?.textContent === "Review"),
      "the Review screen",
    );
    for (const id of [original.id, follower.id, original.id]) {
      await openId(id);
      await browser.$("button*=Commands").click();
      await opened(`button after opening ${id}`);
      await run("Refresh the list");
      await closed("the command ran");
      await ctrlK(`Ctrl+K after opening ${id}`);
      await escape("Esc");
    }
  });

  it("opens after a request opened by ID, Esc and a search of every repository", async () => {
    await press(browser, ["Control", "2"], () => browser.$('[aria-label="Search requests"]').isExisting(), "this case's Review screen");
    const original = requests(fixture).find((request) => request.evalId === "release/signoff");
    assert.ok(original);
    const listed = () =>
      browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
    for (let round = 0; round < 2; round += 1) {
      await openId(original.id);
      // Esc reaches the review only once focus is inside it.
      await eventually(
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
      await typeText(browser, "kind:human status:waiting_human ");
      await eventually(async () => (await listed()) === 2, "the sign-offs of both repositories");
      await press(browser, ["Enter"], focused("Requests"), "focus on the list");
      await browser.$("button*=Commands").click();
      await opened(`button after the search, round ${round}`);
      await run("Refresh the list");
      await closed("the command ran");
      await ctrlK(`Ctrl+K after the search, round ${round}`);
      await run("Show waiting Human reviews");
      await closed("the search reset");
      await eventually(async () => (await listed()) === 1, "this repository's sign-off again");
    }
  });

  it("opens again after a click outside closed it", async () => {
    await ctrlK("Ctrl+K");
    await browser.action("pointer").move({ x: 20, y: 600 }).down().up().perform();
    await closed("outside click");
    await ctrlK("after the outside click");
    await escape("Esc");
    assert.deepEqual(await browser.execute(() => (window as unknown as { errors: string[] }).errors), []);
  });
});
