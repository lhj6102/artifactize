// Moving through the Monitor in the real app: the keys help, a refresh, folding the Run tree,
// jumping to what needs attention and to an upstream blocker, and stepping back out of a
// Detail; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { artifactize, create, type Fixture } from "./fixture.ts";
import { type Session, start } from "./driver.ts";
import { press } from "./keys.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("monitor navigation", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);
  const selected = () => browser.$('[role="treeitem"][aria-selected="true"]').getText();
  /** The tree row of an Artifact (level 1) or of an eval (level 2) by its name. */
  const row = (level: 1 | 2, name: string) =>
    browser.$(`//li[@role="treeitem"][@aria-level="${level}"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "${name}")]]`);
  const helpOpen = () => browser.execute(() => !!document.querySelector('[role="dialog"]'));

  before(async () => {
    fixture = create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-HELP-OPEN E-HELP-CLOSE
  it("lists the keys of the screen with ? and closes the list with Esc", async () => {
    await browser.$('[role="tree"]').click();
    await press(browser, ["?"], helpOpen, "the keys dialog");
    const shown = await text();
    assert.match(shown, /Keyboard shortcuts/);
    assert.match(shown, /Upstream blocker/, "the Monitor's own keys are listed");
    await press(browser, ["Escape"], async () => !(await helpOpen()), "the keys dialog closed");
  });

  // UX edges: E-REFRESH
  it("reads the state again with F5", async () => {
    // A Run another process starts is listed after a refresh at the latest.
    assert.equal(artifactize(fixture, "verify", "--all", "--timeout-ms", "1", "--json"), 3);
    await press(browser, ["F5"], async () => /Runs \(2\)/i.test(await text()), "the second Run listed");
  });

  // UX edges: E-MON-TREE-FOLD
  it("folds and unfolds an Artifact's evals with h, l and Space", async () => {
    const release = await row(1, "release");
    await release.click();
    const expanded = async () => (await row(1, "release")).getAttribute("aria-expanded");
    assert.equal(await expanded(), "true");
    await press(browser, ["h"], async () => (await expanded()) === "false", "release folded by h");
    await press(browser, ["l"], async () => (await expanded()) === "true", "release unfolded by l");
    await press(browser, [" "], async () => (await expanded()) === "false", "release folded by Space");
    await press(browser, [" "], async () => (await expanded()) === "true", "release unfolded by Space");
  });

  // UX edges: E-MON-ATTENTION
  it("jumps to the next eval that needs attention with !", async () => {
    await (await row(1, "docs")).click();
    assert.doesNotMatch(await selected(), /signoff/);
    await press(browser, ["!"], async () => /signoff/.test(await selected()), "the waiting sign-off selected");
  });

  // UX edges: E-MON-BLOCKER
  it("jumps from a blocked eval to the upstream Artifact it waits for with b", async () => {
    await (await row(2, "check")).click();
    const level = () => browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("aria-level");
    await press(
      browser,
      ["b"],
      async () => (await level()) === "1" && /release/.test(await selected()),
      "the upstream Artifact selected",
    );
  });

  // UX edges: E-MON-DETAIL-BACK
  it("steps back from an opened Detail to the tree", async () => {
    await (await row(2, "check")).click();
    const close = () => browser.$("button*=Close");
    await press(browser, ["Enter"], () => close().isExisting(), "the Detail opened");
    await press(browser, ["Escape"], async () => !(await close().isExisting()), "the Detail closed");
    await eventually(() => browser.$('[role="tree"]').isFocused(), "focus back on the tree");
  });
});

describe("quitting without a claim", () => {
  let session: Session;

  before(async () => {
    session = await start(create());
    await session.browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-QUIT-CLEAN
  it("closes the window at once with q", async () => {
    const browser = session.browser;
    await browser.$('[role="tree"]').click();
    // The window closes while the key is still being delivered.
    await browser.keys(["q"]).catch(() => {});
    // The window is gone: the driver's session can no longer answer.
    await eventually(async () => {
      try {
        await browser.getTitle();
        return false;
      } catch {
        return true;
      }
    }, "the window closed");
  });
});
