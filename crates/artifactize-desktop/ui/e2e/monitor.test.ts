// The monitor end to end, in the real app on fixture state made by the real CLI: catalog →
// Run tree → Detail, the shared Human Detail's claim, release, tool, RED validation, Korean
// text and GREEN submission, and a live update from another process.
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { artifactize, create, createNavigationFixture, type Fixture, launched, signoff, slowTool } from "./fixture.ts";
import { checkBuild, type Session, start, windowClosed } from "./driver.ts";
import { afterRender, press } from "./keys.ts";
import { checkpoint, commandsRendered } from "./commands.ts";

/** Wait until `check` holds, polling the app; WebKit renders asynchronously. */
async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("monitor", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);
  const human = () => browser.$('[aria-label="Human review"]');
  const toolOutput = () => browser.execute(() => {
    const detail = document.querySelector('[aria-label="Human review"]');
    const notice = detail?.querySelector(".notice");
    const output = detail?.querySelector('[aria-label="Tool output"]');
    const title = output?.querySelector(".box-title");
    return {
      notice: notice?.textContent?.trim() ?? "",
      noticeError: notice?.classList.contains("error") ?? false,
      title: title?.textContent?.trim() ?? "",
      outputError: title?.classList.contains("error") ?? false,
      text: output?.querySelector("pre")?.textContent ?? "",
    };
  });
  async function openReview(claim = true) {
    await browser.$('//li[@role="treeitem"][@aria-level="2"][.//span[starts-with(normalize-space(.), "signoff")]]').click();
    await press(browser, ["Enter"], () => human().isExisting(), "this case's Human Detail");
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    if (claim) {
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && (await text()).includes("REVIEW (yours)"), "this case's claim");
    }
  }

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  afterEach(async () => {
    await session?.dispose();
  });

  // UX edges: E-LAUNCH-MONITOR
  it("catalogs the repository and its Run, and peeks at the selected eval", async () => {
    const shown = await text();
    assert.equal(await browser.$(".brand").getText(), "Artifactize");
    await eventually(() => browser.execute(() => {
      const mark = document.querySelector<HTMLImageElement>(".brand img");
      return !!mark && mark.complete && mark.naturalWidth > 0;
    }), "the approved brand image finished loading successfully");
    assert.equal(await browser.$(".brand img").execute((image) => {
      const mark = image as HTMLImageElement;
      return mark.complete && mark.naturalWidth > 0;
    }), true, "the approved brand mark actually loaded");
    assert.match(shown, /ALL/);
    assert.match(shown, /master · repo/);
    assert.match(shown, /Runs \(1\)/i);
    assert.match(shown, /RUNNING/);
    assert.match(shown, /validation pending/);
    // A new Run starts on its first in-progress eval; the peek follows it.
    const selected = await browser.$('[role="treeitem"][aria-selected="true"]').getText();
    assert.match(selected, /signoff/);
    assert.match(shown, /WAITING_HUMAN — waiting for a Human submission/);
    const activity = (await checkpoint(browser)).activity;
    assert.ok(activity.commands.launch?.completed > 0, "the window observes its normal launch completion");
    assert.ok(activity.commands.browse?.completed > 0, "the AppState Browse helper reports to the same owner");
  });

  // UX edges: E-HUMAN-CLAIMED-ELSEWHERE E-HUMAN-RELEASED-ELSEWHERE E-HUMAN-CLAIM E-HUMAN-RELEASE E-MON-LIVE
  it("follows a claim another process makes, then claims and releases explicitly", async () => {
    await openReview(false);
    const id = signoff(fixture).id;
    assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("claimed by bob · read-only"), "bob's claim", 12000);
    assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("unclaimed"), "bob's release", 12000);

    // A composing keystroke (an input method's) is never a shortcut.
    await human().click();
    const claimBefore = (await checkpoint(browser)).activity.commands.humanClaim;
    const dispatched = claimBefore?.started ?? 0;
    await browser.execute(() => {
      const target = document.querySelector('[aria-label="Human review"]');
      target?.dispatchEvent(new KeyboardEvent("keydown", { key: "c", isComposing: true, bubbles: true }));
    });
    await afterRender(browser);
    assert.equal((await checkpoint(browser)).activity.commands.humanClaim?.started ?? 0, dispatched,
      "a composition dispatches no claim, including one whose native completion is still pending");
    assert.equal(signoff(fixture).claim, null, "a composition claims nothing");

    await browser.keys(["c"]);
    await eventually(async () => (await text()).includes("claimed by you (alice)"), "claimed");
    assert.equal(signoff(fixture).claim?.reviewer, "alice");
    const claimed = (await checkpoint(browser)).activity.commands.humanClaim;
    assert.ok(claimed && claimed.completed > (claimBefore?.completed ?? 0), "the same read-only probe observes this successful claim");
    assert.equal(claimed.pending, 0);
    await browser.keys(["u"]);
    await eventually(async () => (await text()).includes("Claim released."), "released");
    assert.equal(signoff(fixture).claim, null);
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice", "claimed again");
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
  });

  // UX edges: E-HUMAN-TOOL
  it("runs an output tool and shows its output", async () => {
    await openReview();
    const notes = await browser.$('//li[@role="option"][.//span[text()="notes_release"]]');
    await notes.click();
    await eventually(async () => (await notes.getAttribute("aria-selected")) === "true", "notes selected");
    assert.doesNotMatch((await toolOutput()).text, /Ready to ship\./, "this case has not produced the notes output yet");
    await press(browser, ["Enter"], async () => {
      const shown = await toolOutput();
      return shown.title === "Output · notes_release" && shown.notice === "notes_release finished." && /Ready to ship\./.test(shown.text);
    }, "the selected notes tool's completed output and notice");
    const shown = await toolOutput();
    assert.equal(shown.title, "Output · notes_release");
    assert.match(shown.text, /Ready to ship\./);
    assert.equal(shown.notice, "notes_release finished.");
    assert.equal(shown.outputError, false);
    assert.equal(shown.noticeError, false);
    const saved = signoff(fixture);
    assert.equal(saved.status, "WAITING_HUMAN", "tool output is not a Human verdict");
    assert.equal(saved.claim?.reviewer, "alice");
    assert.ok(await human().isExisting(), "the same Human Detail remains open");
  });

  // UX edges: E-HUMAN-TOOL-LAUNCH
  it("runs a launch tool and says the application was opened", async () => {
    await openReview();
    const open = await browser.$('//li[@role="option"][.//span[text()="open_release"]]');
    await open.click();
    await eventually(async () => (await open.getAttribute("aria-selected")) === "true", "open selected");
    await browser.keys(["Enter"]);
    await eventually(async () => /open_release (launched|started|opened)/i.test(await text()), "the launch notice");
    // The application stand-in really ran.
    await eventually(() => launched(fixture), "the launched application");
  });

  // UX edges: E-HUMAN-TOOL-FAIL
  it("shows the error of a tool that fails", async () => {
    await openReview();
    const fail = await browser.$('//li[@role="option"][.//span[text()="fail_release"]]');
    await fail.click();
    await eventually(async () => (await fail.getAttribute("aria-selected")) === "true", "fail selected");
    const diagnostic = /Human tool exited unsuccessfully \([^)]*\b1\)\./;
    const before = await toolOutput();
    assert.doesNotMatch(before.text, diagnostic, "this case has not produced the failing tool's exit diagnostic yet");
    assert.doesNotMatch(before.notice, /fail_release failed/);
    await press(browser, ["Enter"], async () => {
      const shown = await toolOutput();
      return shown.noticeError && shown.outputError &&
        shown.notice === "fail_release failed; a tool error is not a verdict." &&
        shown.title === "Output · fail_release · tool error" && diagnostic.test(shown.text);
    }, "the selected failing tool's actual exit diagnostic and error notice");
    const shown = await toolOutput();
    assert.equal(shown.notice, "fail_release failed; a tool error is not a verdict.");
    assert.equal(shown.title, "Output · fail_release · tool error");
    assert.match(shown.text, diagnostic);
    assert.equal(shown.noticeError, true);
    assert.equal(shown.outputError, true);
    const saved = signoff(fixture);
    assert.equal(saved.status, "WAITING_HUMAN", "a failed tool is not a Human verdict");
    assert.equal(saved.result, null);
    assert.equal(saved.claim?.reviewer, "alice", "the same Human review remains claimed");
    assert.ok(await human().isExisting(), "the same Human Detail remains open after the tool error");
  });

  // UX edges: E-HUMAN-ESC-CANCEL
  it("cancels a running tool with Esc", async () => {
    await openReview();
    const slow = await browser.$('//li[@role="option"][.//span[text()="slow_release"]]');
    await slow.click();
    await eventually(async () => (await slow.getAttribute("aria-selected")) === "true", "slow selected");
    await browser.keys(["Enter"]);
    await eventually(async () => /Running slow_release/.test(await text()), "the tool running");
    await eventually(() => slowTool(fixture) !== null, "the owned tool process started");
    const alive = slowTool(fixture);
    assert.ok(alive && alive(), "the cancellation target is an actual running process");
    await browser.keys(["Escape"]);
    await eventually(async () => !/Running slow_release/.test(await text()), "the tool cancelled");
    await eventually(() => !alive(), "Esc terminated the owned tool process");
    assert.ok(await human().isExisting(), "Esc cancelled the job and left the Detail open");
  });

  // UX edges: E-HUMAN-RED E-HUMAN-INVALID E-HUMAN-SWITCH
  it("keeps a RED result that fails its schema correctable and shows the failing path", async () => {
    await openReview();
    await browser.$('[aria-label="Human review"]').click();
    await browser.keys(["r"]);
    const reason = await browser.$("#field-reason");
    await reason.waitForExist();
    await reason.setValue("no");
    await browser.keys(["Control", "s"]);
    const alert = await browser.$('[role="alert"]');
    await alert.waitForExist({ timeout: 15000 });
    assert.match(await alert.getText(), /reason/);
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    // The form keeps what was entered and stays editable, to be corrected.
    assert.equal(await reason.getValue(), "no");
    await reason.setValue("not good enough");
    assert.equal(await reason.getValue(), "not good enough");
    assert.equal(signoff(fixture).result, null, "the invalid RED result was not saved");
    // Continue this rejected submission, without leaving its claimed review. Each verdict
    // must rebuild the owner's corresponding schema, even after validation failed.
    await press(browser, ["Control", "g"], async () =>
      await browser.$("#field-comment").isExisting() && !(await reason.isExisting()), "the pass-schema form after invalid RED");
    assert.equal(await browser.$("#field-approved").getValue(), "true (fixed)", "the pass schema's approved constant");
    await browser.$("#field-comment").setValue("checked after rejection");
    assert.equal(await browser.$("#field-comment").getValue(), "checked after rejection");
    await press(browser, ["Control", "r"], async () =>
      await reason.isExisting() && !(await browser.$("#field-comment").isExisting()), "the fail-schema form again");
    await reason.setValue("a correctable reason");
    assert.equal(await reason.getValue(), "a correctable reason");
    assert.equal(await browser.$("#field-approved").isExisting(), false, "no pass-schema field in RED");
    const saved = signoff(fixture);
    assert.equal(saved.status, "WAITING_HUMAN");
    assert.equal(saved.result, null, "switching did not submit the rejected draft");
    assert.equal(saved.claim?.reviewer, "alice", "the same review remains claimed and editable");
  });

  // UX edges: E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  it("submits GREEN with Korean text edited as JSON and keeps the completed result open", async () => {
    await openReview();
    await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
    const comment = await browser.$("#field-comment");
    await comment.waitForExist();
    // An input method's Escape (composing, keyCode 229) belongs to the composition.
    await browser.execute(() => {
      const field = document.querySelector("#field-comment");
      field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true }));
      field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", keyCode: 229, bubbles: true }));
    });
    assert.ok(await comment.isExisting(), "the form stays open");
    const korean = "릴리스 노트를 확인했습니다";
    await comment.setValue(korean);
    assert.equal(await comment.getValue(), korean);
    // Keyboard only: Ctrl+E turns the typed form into JSON with focus in it, the JSON is
    // edited in place, and Ctrl+S submits it; no click in between.
    await browser.keys(["Control", "e"]);
    await eventually(
      () => browser.execute(() => document.activeElement?.matches("textarea.json") ?? false),
      "focus in the JSON text",
    );
    const json = await browser.$("textarea.json");
    assert.deepEqual(JSON.parse(await json.getValue()), { approved: true, comment: korean });
    // To the end of the comment line, before its closing quote, and append.
    await browser.keys(["Control", "End"]);
    await browser.keys(["ArrowUp", "End", "ArrowLeft"]);
    await browser.keys([..." (JSON)"]);
    await browser.keys(["Control", "s"]);
    await eventually(async () => signoff(fixture).status === "GREEN", "GREEN saved");
    assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: `${korean} (JSON)` });
    const run = signoff(fixture).runId;
    await eventually(() => browser.execute((id) => {
      const row = [...document.querySelectorAll('.monitor [aria-label="Runs"] [role="option"]')]
        .find((entry) => entry.querySelector(".id")?.textContent?.split(/\s/)[0] === id);
      return row?.querySelector(".status")?.textContent?.trim() === "GREEN";
    }, run), "the app has rendered this Run's terminal GREEN update");
    await commandsRendered(browser);
    assert.ok(await human().isExisting(), "the completed Detail stays open after the terminal subscription update");
    const completed = await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
    assert.match(completed, /completed result/i);
    assert.match(completed, /Submitted GREEN for release\/signoff/);
  });

  // UX edges: E-MON-RESIZE
  it("resizes a split pane by dragging its divider", async () => {
    const width = () =>
      browser.execute(() =>
        (document.querySelector('[role="tree"]')?.closest("[data-pane]") as HTMLElement).getBoundingClientRect().width,
      );
    const before = await width();
    const dividers = await browser.$$("[data-pane-resizer]");
    // Paneforge installs its move listener in a post-press render effect. Keep the real
    // pointer held across that render boundary, rather than guessing how long it takes.
    await browser.action("pointer", { id: "resize" }).move({ origin: dividers[2] }).down().perform(true);
    try {
      await afterRender(browser);
      const pointer = browser.action("pointer", { id: "resize" });
      for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: 20, y: 0 });
      await pointer.perform(true);
      await eventually(async () => (await width()) > before + 60, "the dragged Run tree");
      await browser.action("pointer", { id: "resize" }).up().perform();
    } finally { await browser.releaseActions(); }
    await eventually(async () => (await width()) > before + 60, "a wider Run tree");
  });
});

describe("a continuous Monitor keyboard journey", () => {
  let example: Awaited<ReturnType<typeof createNavigationFixture>>;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const pane = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);
  const selectedRun = () => browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");
  const selectedTree = () => browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("data-path");
  const scopeIndex = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')]
    .findIndex((row) => row.getAttribute("aria-selected") === "true"));

  beforeEach(async () => {
    checkBuild();
    example = await createNavigationFixture();
    session = await start(example.fixture);
    browser = session.browser;
    await eventually(() => pane("Runs").isFocused(), "the initial Runs pane focused");
  });

  afterEach(async () => { await session?.dispose(); });

  // UX edges: E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-MOVE E-MON-RUNS-TREE
  // E-MON-TREE-MOVE E-MON-TREE-FOLD E-MON-TREE-DETAIL E-MON-DETAIL-BACK
  // E-MON-TREE-TO-RUNS E-MON-RUNS-TO-SCOPE
  it("walks Scope → another Run → tree movement and folds → Detail → Tree → Runs → Scope without a mouse", async () => {
    const [older, newer] = example.runs;
    await press(browser, ["ArrowLeft"], () => pane("Scope").isFocused(), "Scope focused");
    const labels = await browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"] .label')]
      .map((row) => row.textContent?.trim() ?? ""));
    const repo = labels.indexOf("repo");
    assert.ok(repo > 0, "this case's repository follows ALL");
    while ((await scopeIndex()) < repo) {
      const before = await scopeIndex();
      await press(browser, ["ArrowDown"], async () => (await scopeIndex()) === before + 1, "the next scope selected");
    }
    assert.equal(await scopeIndex(), repo);
    const runs = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Runs"] .id')]
      .map((row) => row.textContent?.split(/\s/)[0] ?? "").sort());
    await eventually(async () => JSON.stringify(await runs()) === JSON.stringify([...example.scopes.repo].sort()), "the selected repository's exact Runs");
    await press(browser, ["Enter"], () => pane("Runs").isFocused(), "the repository's Runs focused");
    assert.equal(await selectedRun(), newer.id);
    await press(browser, ["ArrowDown"], async () => (await selectedRun()) === older.id, "another identified Run selected");
    await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "that Run's Tree focused");
    const evalPath = `a:subject/e:subject/${older.eval}`;
    await eventually(async () => (await selectedTree()) === evalPath, "the older Run's own criterion selected");
    await press(browser, ["k"], async () => (await selectedTree()) === "a:subject", "the Artifact above its eval selected");
    const expanded = () => browser.$('[role="treeitem"][data-path="a:subject"]').getAttribute("aria-expanded");
    await press(browser, ["h"], async () => (await expanded()) === "false", "the Artifact folded");
    await press(browser, ["l"], async () => (await expanded()) === "true", "the Artifact unfolded");
    await press(browser, ["j"], async () => (await selectedTree()) === evalPath, "the identified eval selected again");
    const human = () => browser.$('[aria-label="Human review"]');
    await press(browser, ["Enter"], () => human().isExisting(), "the selected eval's Detail opened");
    await eventually(() => browser.execute((name) => document.querySelector('[aria-label="Human review"] h3')?.textContent?.includes(`subject/${name}`) ?? false,
      older.eval), "Detail has loaded the eval selected in this journey");
    await press(browser, ["Escape"], async () => !(await human().isExisting()) && await browser.$('[role="tree"]').isFocused(), "back to Tree");
    assert.equal(await selectedTree(), evalPath, "Tree selection survived the Detail");
    await press(browser, ["Escape"], () => pane("Runs").isFocused(), "back to Runs");
    assert.equal(await selectedRun(), older.id, "the same Run remains selected");
    await press(browser, ["ArrowLeft"], () => pane("Scope").isFocused(), "back to Scope");
    assert.equal(await scopeIndex(), repo, "the same repository scope remains selected");
  });
});

describe("quitting the Monitor with a claim", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  afterEach(async () => {
    await session?.dispose();
  });

  // UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
  it("asks before quitting with a claim taken here: Esc stays, u releases and quits", async () => {
    // The Runs pane has focus first: Enter steps into the tree, which starts on the sign-off.
    await browser.keys(["Enter"]);
    await eventually(() => browser.$('[role="tree"]').isFocused(), "tree focus");
    await browser.keys(["Enter"]);
    await eventually(() => browser.$('[aria-label="Human review"]').isExisting(), "Human Detail");
    await eventually(async () => (await text()).includes("Unclaimed. c claims it for alice"), "the claim stage");
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
    // Back out of the Detail to the tree, then q outside any field.
    await browser.keys(["Escape"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
    const dialog = await browser.$('[role="alertdialog"]');
    await browser.keys(["q"]);
    await dialog.waitForExist({ timeout: 10000 });
    await eventually(
      () => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')),
      "focus in the prompt",
    );
    await browser.keys(["Escape"]);
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    assert.equal(signoff(fixture).claim?.reviewer, "alice", "staying keeps the claim");

    await browser.keys(["q"]);
    await dialog.waitForExist({ timeout: 10000 });
    await eventually(
      () => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')),
      "focus in the prompt again",
    );
    // Confirm the window's actual terminal state, never an arbitrary driver exception.
    await browser.keys(["u"]).catch(async (error: unknown) => { if (!(await windowClosed(browser))) throw error; });
    await eventually(() => signoff(fixture).claim === null, "released on quit");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    await eventually(() => windowClosed(browser), "the window closed after releasing, before fixture teardown");
  });
});
