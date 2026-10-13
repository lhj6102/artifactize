// The monitor end to end, in the real app on fixture state made by the real CLI: catalog →
// Run tree → Detail, the shared Human Detail's claim, release, tool, RED validation, Korean
// text and GREEN submission, and a live update from another process.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { artifactize, create, type Fixture, signoff } from "./fixture.ts";
import { type Session, start } from "./driver.ts";

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

  before(async () => {
    fixture = await create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-LAUNCH-MONITOR
  it("catalogs the repository and its Run, and peeks at the selected eval", async () => {
    const shown = await text();
    assert.match(shown, /ALL/);
    assert.match(shown, /master · repo/);
    assert.match(shown, /Runs \(1\)/i);
    assert.match(shown, /RUNNING/);
    assert.match(shown, /validation pending/);
    // A new Run starts on its first in-progress eval; the peek follows it.
    const selected = await browser.$('[role="treeitem"][aria-selected="true"]').getText();
    assert.match(selected, /signoff/);
    assert.match(shown, /WAITING_HUMAN — waiting for a Human submission/);
  });

  // UX edges: E-MON-SCOPE-RUNS E-MON-RUNS-TREE E-MON-TREE-MOVE E-MON-TREE-DETAIL
  it("navigates Scope → Runs → tree → Detail with the keyboard", async () => {
    await browser.$('[aria-label="Scope"] [role="option"]').click();
    await eventually(async () => (await text()).includes("all repositories"), "ALL selected");
    await browser.keys(["Enter"]);
    await browser.keys(["Enter"]);
    await eventually(() => browser.$('[role="tree"]').isFocused(), "tree focus");
    // Move to the docs Artifact and back to the sign-off.
    await browser.keys(["j"]);
    await eventually(async () => /ship|check/.test(await browser.$('[role="treeitem"][aria-selected="true"]').getText()), "moved down");
    await browser.keys(["k"]);
    await eventually(async () => /signoff/.test(await browser.$('[role="treeitem"][aria-selected="true"]').getText()), "back on signoff");
    await browser.keys(["Enter"]);
    await eventually(() => human().isExisting(), "Human Detail");
    const crumbs = await browser.$('nav[aria-label="Breadcrumb"]').getText();
    assert.match(crumbs, /artifactize\s*›\s*all repositories\s*›\s*run-\S+\s*›\s*signoff/);
  });

  // UX edges: E-HUMAN-CLAIMED-ELSEWHERE E-HUMAN-RELEASED-ELSEWHERE E-HUMAN-CLAIM E-HUMAN-RELEASE E-MON-LIVE
  it("follows a claim another process makes, then claims and releases explicitly", async () => {
    const id = signoff(fixture).id;
    assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("claimed by bob · read-only"), "bob's claim", 12000);
    assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
    await eventually(async () => (await text()).includes("unclaimed"), "bob's release", 12000);

    // A composing keystroke (an input method's) is never a shortcut.
    await human().click();
    await browser.execute(() => {
      const target = document.querySelector('[aria-label="Human review"]');
      target?.dispatchEvent(new KeyboardEvent("keydown", { key: "c", isComposing: true, bubbles: true }));
    });
    await browser.pause(500);
    assert.equal(signoff(fixture).claim, null, "a composition claims nothing");

    await browser.keys(["c"]);
    await eventually(async () => (await text()).includes("claimed by you (alice)"), "claimed");
    assert.equal(signoff(fixture).claim?.reviewer, "alice");
    await browser.keys(["u"]);
    await eventually(async () => (await text()).includes("Claim released."), "released");
    assert.equal(signoff(fixture).claim, null);
    await browser.keys(["c"]);
    await eventually(async () => signoff(fixture).claim?.reviewer === "alice", "claimed again");
    await eventually(async () => (await text()).includes("REVIEW (yours)"), "the review stage");
  });

  // UX edges: E-HUMAN-TOOL
  it("runs an output tool and shows its output", async () => {
    const notes = await browser.$('//li[@role="option"][.//span[text()="notes_release"]]');
    await notes.click();
    await eventually(async () => (await notes.getAttribute("aria-selected")) === "true", "notes selected");
    await browser.keys(["Enter"]);
    await eventually(async () => (await text()).includes("Ready to ship."), "tool output");
    assert.match(await text(), /notes_release finished\./);
  });

  // UX edges: E-HUMAN-TOOL-LAUNCH
  it("runs a launch tool and says the application was opened", async () => {
    const open = await browser.$('//li[@role="option"][.//span[text()="open_release"]]');
    await open.click();
    await eventually(async () => (await open.getAttribute("aria-selected")) === "true", "open selected");
    await browser.keys(["Enter"]);
    await eventually(async () => /open_release (launched|started|opened)/i.test(await text()), "the launch notice");
    // The application stand-in really ran.
    await eventually(() => existsSync(join(fixture.root, "launched")), "the launched application");
  });

  // UX edges: E-HUMAN-TOOL-FAIL
  it("shows the error of a tool that fails", async () => {
    const fail = await browser.$('//li[@role="option"][.//span[text()="fail_release"]]');
    await fail.click();
    await eventually(async () => (await fail.getAttribute("aria-selected")) === "true", "fail selected");
    await browser.keys(["Enter"]);
    await eventually(
      () => browser.execute(() => !!document.querySelector(".notice.error, [aria-label='Tool output'] .error")),
      "the tool's error",
    );
    assert.match(await text(), /fail_release/);
  });

  // UX edges: E-HUMAN-ESC-CANCEL
  it("cancels a running tool with Esc", async () => {
    const slow = await browser.$('//li[@role="option"][.//span[text()="slow_release"]]');
    await slow.click();
    await eventually(async () => (await slow.getAttribute("aria-selected")) === "true", "slow selected");
    await browser.keys(["Enter"]);
    await eventually(async () => /Running slow_release/.test(await text()), "the tool running");
    await browser.keys(["Escape"]);
    await eventually(async () => !/Running slow_release/.test(await text()), "the tool cancelled");
    assert.ok(await human().isExisting(), "Esc cancelled the job and left the Detail open");
  });

  // UX edges: E-HUMAN-RED E-HUMAN-INVALID
  it("keeps a RED result that fails its schema correctable and shows the failing path", async () => {
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
  });

  // UX edges: E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  it("submits GREEN with Korean text edited as JSON and keeps the completed result open", async () => {
    await browser.keys(["Control", "g"]);
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
    await eventually(async () => /completed result/i.test(await text()), "completed Detail");
    assert.match(await text(), /Submitted GREEN for release\/signoff/);
  });

  // UX edges: E-MON-RESIZE
  it("resizes a split pane by dragging its divider", async () => {
    const width = () =>
      browser.execute(() =>
        (document.querySelector('[role="tree"]')?.closest("[data-pane]") as HTMLElement).getBoundingClientRect().width,
      );
    const before = await width();
    const dividers = await browser.$$("[data-pane-resizer]");
    // Scope | Runs, the sidebar | Run tree, then the Run tree | Detail divider. paneforge
    // listens for moves only from an effect after the press starts the drag, so the pointer
    // pauses after the press and moves in steps, as a hand does; one instant move (what
    // dragAndDrop sends) could arrive before the listener and drop the whole drag.
    const pointer = browser.action("pointer");
    pointer.move({ origin: dividers[2] }).down().pause(150);
    for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: 20, y: 0, duration: 20 });
    await pointer.pause(100).up().perform();
    await eventually(async () => (await width()) > before + 60, "a wider Run tree");
  });
});

describe("quitting the Monitor with a claim", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  before(async () => {
    fixture = await create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
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
    // Releasing quits: the window, and with it the WebDriver session, may end mid-keystroke.
    await browser.keys(["u"]).catch(() => {});
    await eventually(() => signoff(fixture).claim === null, "released on quit");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
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
