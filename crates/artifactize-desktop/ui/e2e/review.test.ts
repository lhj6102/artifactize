// The standalone review end to end: the waiting list, opening a request by ID, and quitting
// with an unsubmitted claim (stay, then release and quit).
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { artifactize, create, type Fixture, signoff } from "./fixture.ts";
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

  // UX edges: E-REVIEW-PREVIEW
  it("lists the waiting sign-off with a read-only preview", async () => {
    const list = await browser.$('[aria-label="Requests"]');
    await list.waitForExist({ timeout: 15000 });
    // WebKitWebDriver's element text skips these grid rows; read what the page renders.
    const shown = () => browser.execute((element) => (element as HTMLElement).innerText, list);
    await eventually(async () => (await shown()).includes("release/signoff"), "the waiting row");
    await eventually(async () => (await text()).includes("Enter or click: open review"), "the preview");
    assert.equal(signoff(fixture).claim, null, "previewing claims nothing");

    // Another process claims it: the list and the Preview both show the new claimant.
    const id = signoff(fixture).id;
    assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("claimed by bob · read-only"), "the Preview's claim");
    assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("WAITING_HUMAN · unclaimed"), "the Preview's release");
  });

  // UX edges: E-REVIEW-OPEN-ID E-HUMAN-CLAIM
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

  // UX edges: E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL
  it("leaves a field's form with Esc, then the Detail with the next Esc, keyboard only", async () => {
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
    await browser.keys(["g"]);
    await eventually(
      () => browser.execute(() => document.activeElement?.id === "field-comment"),
      "focus in the comment field",
    );
    await browser.keys(["Escape"]);
    await browser.$("#field-comment").waitForExist({ reverse: true, timeout: 10000 });
    assert.ok(await browser.$('[aria-label="Human review"]').isExisting(), "the Detail stays");
    await browser.keys(["Escape"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
    // Back on the list: Enter opens the same request again, its claim kept.
    await browser.keys(["Enter"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 10000 });
  });

  // UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
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

    // The window's own close requests, twice in a row: both wait for the prompt's answer.
    await browser.execute(() => {
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: object) => Promise<unknown> } })
        .__TAURI_INTERNALS__;
      void internals.invoke("plugin:window|close", { label: "main" });
      void internals.invoke("plugin:window|close", { label: "main" });
    });
    await dialog.waitForExist({ timeout: 10000 });
    await browser.pause(500);
    assert.equal(await browser.execute(() => document.title), "Artifactize", "the window stays");
    assert.equal(signoff(fixture).claim?.reviewer, "alice");
    await eventually(
      () => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')),
      "focus in the prompt",
    );
    await browser.keys(["Escape"]);
    await dialog.waitForExist({ reverse: true, timeout: 10000 });

    await prompted();
    // Releasing quits: the window, and with it the WebDriver session, may end mid-keystroke.
    await browser.keys(["u"]).catch(() => {});
    await eventually(() => signoff(fixture).claim === null, "released on quit");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    // And the window is gone: the driver's session can no longer answer.
    await eventually(async () => {
      try {
        await browser.getTitle();
        return false;
      } catch {
        return true;
      }
    }, "the window closed after releasing");
  });
});

describe("a GREEN submission on the Review screen", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  before(async () => {
    fixture = create();
    session = await start(fixture, "review");
    browser = session.browser;
    await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN E-HUMAN-SUBMIT
  it("keeps the completed result open until the reviewer leaves it", async () => {
    await browser.keys(["Control", "o"]);
    const input = await browser.$('[aria-label="Request ID"]');
    await input.waitForExist();
    await input.setValue(signoff(fixture).id);
    await browser.keys(["Enter"]);
    const human = await browser.$('[aria-label="Human review"]');
    await human.waitForExist({ timeout: 15000 });
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
    await browser.keys(["g"]);
    const comment = await browser.$("#field-comment");
    await comment.waitForExist({ timeout: 10000 });
    await comment.click();
    await browser.keys([..."looks good"]);
    await browser.keys(["Control", "s"]);
    await eventually(async () => signoff(fixture).status === "GREEN", "GREEN saved");
    await eventually(async () => /completed result/i.test(await text()), "the completed result");
    await browser.pause(1000);
    assert.ok(await human.isExisting(), "the completed Detail stays open");
    assert.match(await text(), /completed result/i);
  });
});
