// The standalone review end to end: the waiting list, opening a request by ID, and quitting
// with an unsubmitted claim (stay, then release and quit).
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { addRepository, artifactize, create, type Fixture, requests, signoff } from "./fixture.ts";
import { press, typeText } from "./keys.ts";
import { checkBuild, type Session, start, windowClosed } from "./driver.ts";
import { commandsRendered } from "./commands.ts";

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

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture, "review");
    browser = session.browser;
  });

  afterEach(async () => {
    await session?.dispose();
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
  it("opens the entered request ID rather than the selected request and claims it", async () => {
    await browser.$('[aria-label="Requests"] [aria-selected="true"]').waitForExist({ timeout: 15000 });
    const original = signoff(fixture).id;
    const before = new Set(requests(fixture).map((request) => request.id));
    await addRepository(fixture, "open-by-id");
    const added = requests(fixture).find((request) => request.evalId === "release/signoff" && !before.has(request.id));
    assert.ok(added, "the other repository created its own sign-off");
    const otherId = added.id;
    assert.notEqual(otherId, original, "different repositories own independent reviews, not followers");
    await eventually(() => browser.execute((ids) => {
      const listed = [...document.querySelectorAll('[aria-label="Requests"] .mono')].map((cell) => cell.textContent);
      return ids.every((id) => listed.includes(id));
    }, [original, otherId]), "both independent requests are in this case's list");
    await commandsRendered(browser);
    // A live insertion may select either row. Read the actual selected identity rather
    // than assuming the older request stayed selected. WebKit getText omits grid cells.
    const selected = await browser.execute(() => document.querySelector('[aria-label="Requests"] [aria-selected="true"] .mono')?.textContent ?? "");
    assert.ok([original, otherId].includes(selected));
    const id = selected === original ? otherId : original;
    assert.notEqual(id, selected, "the typed ID differs from the exact currently selected request");
    await press(browser, ["Control", "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
    await browser.$('[aria-label="Request ID"]').setValue(id);
    await press(browser, ["Enter"], () => browser.$('[aria-label="Human review"]').isExisting(), "the entered review opened");
    const technical = () => browser.$('[aria-label="Human review"]').$("button*=Technical").getAttribute("aria-expanded");
    await press(browser, ["t"], async () => (await technical()) === "true", "the opened review's identity");
    const openedId = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Human review"] dt')]
      .find((field) => field.textContent === "Request")?.nextElementSibling?.textContent ?? "");
    await eventually(async () => (await openedId()) === id, "Detail identifies exactly the entered request");
    assert.notEqual(await openedId(), selected);
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    await press(browser, ["c"], () => requests(fixture).find((request) => request.id === id)?.claim?.reviewer === "alice", "the entered request claimed");
    // request list reaches every repository in this state: observe by exact identities,
    // not by calling a first-signoff helper with another --repo path.
    const previous = requests(fixture).find((request) => request.id === selected);
    assert.ok(previous, "the previously selected request is still saved");
    assert.equal(previous.claim, null, "the previously selected request was not claimed");
  });

  async function claimReview() {
    await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
    await browser.keys(["Control", "o"]);
    const input = await browser.$('[aria-label="Request ID"]');
    await input.waitForExist();
    await input.setValue(signoff(fixture).id);
    await browser.keys(["Enter"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 15000 });
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice" && (await text()).includes("REVIEW (yours)"), "this case's claim");
  }

  // UX edges: E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL
  it("leaves a field's form with Esc, then the Detail with the next Esc, keyboard only", async () => {
    await claimReview();
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
    await claimReview();
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
    await browser.execute(async () => {
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: object) => Promise<unknown> } })
        .__TAURI_INTERNALS__;
      await Promise.all([
        internals.invoke("plugin:window|close", { label: "main" }),
        internals.invoke("plugin:window|close", { label: "main" }),
      ]);
    });
    await dialog.waitForExist({ timeout: 10000 });
    assert.equal(await browser.execute(() => document.title), "Artifactize", "the window stays");
    assert.equal(signoff(fixture).claim?.reviewer, "alice");
    await eventually(
      () => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')),
      "focus in the prompt",
    );
    await browser.keys(["Escape"]);
    await dialog.waitForExist({ reverse: true, timeout: 10000 });

    await prompted();
    // A disappearing window may end this keystroke's response, but a generic driver
    // failure is not closure. The native helper only accepts a terminal window outcome.
    await browser.keys(["u"]).catch(async (error: unknown) => { if (!(await windowClosed(browser))) throw error; });
    await eventually(() => signoff(fixture).claim === null, "released on quit");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    await eventually(() => windowClosed(browser), "the window closed after releasing, before fixture teardown");
  });
});

describe("a GREEN submission on the Review screen", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture, "review");
    browser = session.browser;
    await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
  });

  afterEach(async () => {
    await session?.dispose();
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
    await typeText(browser, "looks good");
    await browser.keys(["Control", "s"]);
    await eventually(async () => signoff(fixture).status === "GREEN", "GREEN saved");
    assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "looks good" });
    const run = signoff(fixture).runId;
    // Monitor stays mounted while Review is shown. Its exact Run row acknowledges the
    // app's terminal subscription update without leaving this completed Review Detail.
    await eventually(() => browser.execute((id) => {
      const row = [...document.querySelectorAll('.monitor [aria-label="Runs"] [role="option"]')]
        .find((entry) => entry.querySelector(".id")?.textContent?.split(/\s/)[0] === id);
      return row?.querySelector(".status")?.textContent?.trim() === "GREEN";
    }, run), "the app has rendered this Run's terminal GREEN update");
    await commandsRendered(browser);
    assert.ok(await human.isExisting(), "the completed Detail stays open after the terminal subscription update");
    const completed = await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
    assert.match(completed, /completed result/i);
  });
});
