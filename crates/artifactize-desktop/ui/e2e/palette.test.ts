// The command palette in the real app: it opens by its button and by Ctrl+K, runs a command,
// and opens again after every way of closing it, screen switches included.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { create } from "./fixture.ts";
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
  const shown = () =>
    browser.execute(() => !!document.querySelector('[aria-label="Command palette"][data-state="open"]'));
  const opened = (how: string) => eventually(shown, `the palette opens (${how})`);
  const closed = (how: string) => eventually(async () => !(await shown()), `the palette closes (${how})`);
  /** Run the command labelled `label` from the open palette by typing and Enter. */
  const run = async (label: string) => {
    await browser.keys([...label]);
    await browser.keys(["Enter"]);
  };

  before(async () => {
    session = await start(create());
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
