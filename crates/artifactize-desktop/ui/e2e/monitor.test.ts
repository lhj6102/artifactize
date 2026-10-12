// The monitor end to end, in the real app on fixture state made by the real CLI: catalog →
// Run tree → Detail, the shared Human Detail's claim, release, tool, RED validation, Korean
// text and GREEN submission, and a live update from another process.
import assert from "node:assert/strict";
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
    fixture = create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  it("catalogs the repository and its Run, and peeks at the selected eval", async () => {
    const shown = await text();
    assert.match(shown, /ALL/);
    assert.match(shown, /master · repo/);
    assert.match(shown, /Runs \(1\)/i);
    assert.match(shown, /NOT SATISFIED at Run end/);
    // A new Run starts on its first in-progress eval; the peek follows it.
    const selected = await browser.$('[role="treeitem"][aria-selected="true"]').getText();
    assert.match(selected, /signoff/);
    assert.match(shown, /WAITING_HUMAN — waiting for a Human submission/);
  });

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

  it("runs an output tool and shows its output", async () => {
    const notes = await browser.$('//li[@role="option"][.//span[text()="notes_release"]]');
    await notes.click();
    await eventually(async () => (await notes.getAttribute("aria-selected")) === "true", "notes selected");
    await browser.keys(["Enter"]);
    await eventually(async () => (await text()).includes("Ready to ship."), "tool output");
    assert.match(await text(), /notes_release finished\./);
  });

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
  });

  it("submits GREEN with Korean text and keeps the completed result open", async () => {
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
    await browser.keys(["Control", "s"]);
    await eventually(async () => signoff(fixture).status === "GREEN", "GREEN saved");
    assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: korean });
    await eventually(async () => /completed result/i.test(await text()), "completed Detail");
    assert.match(await text(), /Submitted GREEN for release\/signoff/);
  });

  it("resizes a split pane by dragging its divider", async () => {
    const width = () =>
      browser.execute(() =>
        (document.querySelector('[role="tree"]')?.closest("[data-pane]") as HTMLElement).getBoundingClientRect().width,
      );
    const before = await width();
    const dividers = await browser.$$("[data-pane-resizer]");
    // Scope | Runs, the sidebar | Run tree, then the Run tree | Detail divider.
    await dividers[2].dragAndDrop({ x: 120, y: 0 });
    await eventually(async () => (await width()) > before + 60, "a wider Run tree");
  });
});
