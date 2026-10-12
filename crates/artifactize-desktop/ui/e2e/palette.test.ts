// The command palette in the real app: it opens by its button and by Ctrl+K, runs a command,
// and opens again after every way of closing it, screen switches included.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { addRepository, artifactize, create, type Fixture, requests } from "./fixture.ts";
import { type Session, start } from "./driver.ts";

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
  /**
   * Type `text` into the focused field one key at a time and check it arrived: under load
   * WebKitWebDriver can drop a key of a burst, which a person's typing never does. A field
   * that lost a key is cleared and typed again.
   */
  const type = async (text: string) => {
    const value = () => browser.execute(() => (document.activeElement as HTMLInputElement | null)?.value ?? "");
    for (let attempt = 0; attempt < 3; attempt += 1) {
      for (const key of text) await browser.keys([key]);
      if ((await value()).endsWith(text)) return;
      await browser.execute(() => {
        const field = document.activeElement as HTMLInputElement | null;
        if (!field) return;
        field.value = "";
        field.dispatchEvent(new Event("input", { bubbles: true }));
      });
    }
    assert.fail(`typed "${text}" but the field holds "${await value()}"`);
  };
  /** Run the command labelled `label` from the open palette by typing and Enter. */
  const run = async (label: string) => {
    await type(label);
    await browser.keys(["Enter"]);
  };

  before(async () => {
    fixture = create();
    // A second Run joins the waiting sign-off: its request is a follower of the first one.
    assert.equal(artifactize(fixture, "verify", "--all", "--timeout-ms", "1", "--json"), 3);
    // Another repository with the same eval IDs, which the review search reaches.
    addRepository(fixture, "other");
    session = await start(fixture);
    browser = session.browser;
    await browser.$("button*=Commands").waitForExist({ timeout: 15000 });
    // Errors the page throws (a broken palette threw while rendering) fail the suite.
    await browser.execute(() => {
      const errors: string[] = [];
      (window as unknown as { errors: string[] }).errors = errors;
      window.addEventListener("error", (event) => errors.push(event.message));
    });
  });

  after(async () => {
    await session?.stop();
  });

  it("opens by its button and by Ctrl+K, and again after Esc, several times", async () => {
    for (let round = 0; round < 3; round += 1) {
      await browser.$("button*=Commands").click();
      await opened("button");
      await browser.keys(["Escape"]);
      await closed("Esc");
      await browser.keys(["Control", "k"]);
      await opened("Ctrl+K");
      await browser.keys(["Escape"]);
      await closed("Esc");
    }
  });

  it("runs commands that switch screens and opens again after each", async () => {
    for (const label of ["Show Review", "Show Monitor", "Show Review", "Show Monitor"]) {
      await browser.keys(["Control", "k"]);
      await opened(`before ${label}`);
      await run(label);
      await closed(label);
      const screen = label === "Show Review" ? "Review" : "Monitor";
      await eventually(
        () => browser.execute((name) => document.querySelector('[role="tab"][data-state="active"]')?.textContent === name, screen),
        `${screen} shows`,
      );
      await browser.$("button*=Commands").click();
      await opened(`after ${label}`);
      await browser.keys(["Escape"]);
      await closed("Esc");
    }
  });

  it("opens after requests were opened by ID on the review screen, a follower included", async () => {
    const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
    const source = (request: (typeof signoffs)[number]) =>
      (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId;
    const original = signoffs.find((request) => source(request) === request.id);
    const follower = signoffs.find((request) => source(request) !== request.id);
    assert.ok(original && follower, "an original and its follower");
    await browser.keys(["Control", "2"]);
    await eventually(
      () => browser.execute(() => document.querySelector('[role="tab"][data-state="active"]')?.textContent === "Review"),
      "the Review screen",
    );
    for (const id of [original.id, follower.id, original.id]) {
      await browser.keys(["Control", "o"]);
      const input = await browser.$('[aria-label="Request ID"]');
      await input.waitForExist();
      await input.setValue(id);
      await browser.keys(["Enter"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 10000 });
      await browser.$("button*=Commands").click();
      await opened(`button after opening ${id}`);
      await run("Refresh the list");
      await closed("the command ran");
      await browser.keys(["Control", "k"]);
      await opened(`Ctrl+K after opening ${id}`);
      await browser.keys(["Escape"]);
      await closed("Esc");
    }
  });

  it("opens after a request opened by ID, Esc and a search of every repository", async () => {
    const original = requests(fixture).find((request) => request.evalId === "release/signoff");
    assert.ok(original);
    const listed = () =>
      browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
    for (let round = 0; round < 2; round += 1) {
      await browser.keys(["Control", "o"]);
      const input = await browser.$('[aria-label="Request ID"]');
      await input.waitForExist();
      await input.setValue(original.id);
      await browser.keys(["Enter"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 10000 });
      await browser.keys(["Escape"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
      // The launch searches its own repository; a search without the repo: term finds both.
      await browser.keys(["/"]);
      await eventually(
        () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Search requests"),
        "focus in the search",
      );
      await browser.keys(["Escape"]);
      // A space ends the term and closes the suggestions, so Enter leaves the field.
      await type("kind:human status:waiting_human ");
      await eventually(async () => (await listed()) === 2, "the sign-offs of both repositories");
      await browser.keys(["Enter"]);
      await browser.$("button*=Commands").click();
      await opened(`button after the search, round ${round}`);
      await run("Refresh the list");
      await closed("the command ran");
      await browser.keys(["Control", "k"]);
      await opened(`Ctrl+K after the search, round ${round}`);
      await run("Show waiting Human reviews");
      await closed("the search reset");
      await eventually(async () => (await listed()) === 1, "this repository's sign-off again");
    }
  });

  it("opens again after a click outside closed it", async () => {
    await browser.keys(["Control", "k"]);
    await opened("Ctrl+K");
    await browser.action("pointer").move({ x: 20, y: 600 }).down().up().perform();
    await closed("outside click");
    await browser.keys(["Control", "k"]);
    await opened("after the outside click");
    await browser.keys(["Escape"]);
    await closed("Esc");
    assert.deepEqual(await browser.execute(() => (window as unknown as { errors: string[] }).errors), []);
  });
});
