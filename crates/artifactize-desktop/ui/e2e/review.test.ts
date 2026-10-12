// The standalone review end to end: the waiting list, opening a request by ID, and quitting
// with an unsubmitted claim (stay, then release and quit).
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { create, type Fixture, signoff } from "./fixture.ts";
import { type Session, start } from "./driver.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("review", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  before(async () => {
    fixture = create();
    session = await start(fixture, "review");
    browser = session.browser;
  });

  after(async () => {
    await session?.stop();
  });

  it("lists the waiting sign-off with a read-only preview", async () => {
    const list = await browser.$('[aria-label="Waiting Human reviews"]');
    await list.waitForExist({ timeout: 15000 });
    // WebKitWebDriver's element text skips these grid rows; read what the page renders.
    const shown = () => browser.execute((element) => (element as HTMLElement).innerText, list);
    await eventually(async () => (await shown()).includes("release/signoff"), "the waiting row");
    await eventually(async () => (await text()).includes("Enter or click: open review"), "the preview");
    assert.equal(signoff(fixture).claim, null, "previewing claims nothing");
  });

  it("opens a request by ID and claims it", async () => {
    const id = signoff(fixture).id;
    await browser.keys(["Control", "o"]);
    const input = await browser.$('[aria-label="Request ID"]');
    await input.waitForExist();
    await input.setValue(id);
    await browser.keys(["Enter"]);
    const human = await browser.$('[aria-label="Human review"]');
    await human.waitForExist({ timeout: 15000 });
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
  });

  it("asks before quitting with the claim: Esc stays, u releases and quits", async () => {
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
    const dialog = await browser.$('[role="alertdialog"]');
    /** The prompt is open and holds focus, so its keys reach it. */
    const prompted = async () => {
      await browser.keys(["q"]);
      await dialog.waitForExist({ timeout: 10000 });
      await eventually(
        () => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')),
        "focus in the prompt",
      );
    };
    await prompted();
    const shown = await browser.execute((element) => (element as HTMLElement).innerText, dialog);
    assert.match(shown, new RegExp(signoff(fixture).id));
    await browser.keys(["Escape"]);
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    assert.equal(signoff(fixture).claim?.reviewer, "alice", "staying keeps the claim");
    await eventually(
      () => browser.execute(() => !!document.activeElement?.closest('[aria-label="Human review"]')),
      "focus back in the review",
    );
    // A `q` within ~100 ms of dismissing the prompt has intermittently ended the WebDriver
    // session (unexplained; see the experiment report). Give the dismissal time to settle.
    await browser.pause(300);

    await prompted();
    await browser.keys(["u"]);
    await eventually(() => signoff(fixture).claim === null, "released on quit");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
  });
});
