// Moving through the Monitor in the real app: the keys help, a refresh, folding the Run tree,
// jumping to what needs attention and to an upstream blocker, and stepping back out of a
// Detail; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { verifyWaiting, create, type Fixture } from "./fixture.ts";
import { checkBuild, type Session, start } from "./driver.ts";
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
    checkBuild();
    fixture = await create();
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

  // UX edges: E-MON-LIVE
  it("shows an identified Run from another process without refreshing", async () => {
    const rows = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')].map((row) => row.innerText));
    const before = await rows();
    const id = await verifyWaiting(fixture);
    assert.ok(before.every((row) => !row.includes(id)), "the new Run was not already listed");
    await eventually(async () => (await rows()).some((row) => row.includes(id)), "the subscribed new Run row appears without F5");
  });

  // UX edges: E-REFRESH
  it("reads the state again with F5 and shows what changed on disk on the same screen", async () => {
    // Count the app's own full reads of the snapshot, which the webview sends as `browse`.
    await browser.execute(() => {
      const page = window as unknown as { reads?: number };
      page.reads = 0;
      const fetched = window.fetch.bind(window);
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
        if (url.endsWith("/browse") && typeof init?.body === "string" && init.body.includes('"full":true')) page.reads = (page.reads ?? 0) + 1;
        return fetched(input, init);
      };
    });
    const reads = () => browser.execute(() => (window as unknown as { reads: number }).reads) as Promise<number>;
    // A change on disk: another Run. F5 right away asks for a full read that shows it.
    await verifyWaiting(fixture);
    const before = await reads();
    await press(browser, ["F5"], async () => (await reads()) > before, "a full read after F5");
    await eventually(async () => /Runs \(3\)/i.test(await text()), "the refreshed third Run listed");
    assert.match(await text(), /Monitor/, "on the same screen");
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

  // UX edges: E-MON-DETAIL-TECHNICAL E-MON-DETAIL-BACK
  it("shows the technical record of an opened Detail with t and steps back to the tree", async () => {
    await (await row(2, "check")).click();
    const close = () => browser.$("button*=Close");
    await press(browser, ["Enter"], () => close().isExisting(), "the Detail opened");
    const technical = () => browser.$("button*=Technical").getAttribute("aria-expanded");
    assert.equal(await technical(), "false");
    await press(browser, ["t"], async () => (await technical()) === "true", "the technical record shown");
    await press(browser, ["t"], async () => (await technical()) === "false", "the technical record hidden");
    await press(browser, ["Escape"], async () => !(await close().isExisting()), "the Detail closed");
    await eventually(() => browser.$('[role="tree"]').isFocused(), "focus back on the tree");
  });
});

describe("the Detail follows the tree's selection", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;
  /** The Detail pane's title: the Monitor's fourth pane (Scope, Runs, Run, Detail). */
  const detail = () =>
    browser.execute(() => document.querySelectorAll("section.panel h2")[3]?.textContent?.trim() ?? "");
  const row = (level: 1 | 2, name: string) =>
    browser.$(`//li[@role="treeitem"][@aria-level="${level}"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "${name}")]]`);

  before(async () => {
    checkBuild();
    session = await start(await create());
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-MON-TREE-MOVE E-MON-TREE-DETAIL
  it("shows the newly selected row after the Detail was opened with Enter", async () => {
    await (await row(2, "check")).click();
    await eventually(async () => /check/.test(await detail()), "the peek of ship/check");
    const close = () => browser.$("button*=Close");
    await press(browser, ["Enter"], () => close().isExisting(), "the Detail opened");
    // The tree takes focus again, by a click on another row...
    await (await row(1, "docs")).click();
    await eventually(async () => /docs/.test(await detail()), "the Detail shows docs after a click");
    // ... and the selection moves by key.
    await press(browser, ["k"], async () => !/docs/.test(await detail()), "the Detail leaves docs after k");
    await press(browser, ["j"], async () => /docs/.test(await detail()), "the Detail returns to docs after j");
  });

  // UX edges: E-MON-TREE-MOVE E-MON-TREE-DETAIL
  it("shows the newly selected row after a click into the Detail", async () => {
    await (await row(2, "check")).click();
    await eventually(async () => /check/.test(await detail()), "the Detail shows ship/check");
    await browser.execute(() => (document.querySelectorAll("section.panel")[3] as HTMLElement).click());
    await (await row(1, "docs")).click();
    await eventually(async () => /docs/.test(await detail()), "the Detail shows docs after a click");
    await (await row(2, "check")).click();
    await eventually(async () => /check/.test(await detail()), "the Detail follows back to ship/check");
  });
});

describe("moving between the panes", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;
  const listbox = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);
  /** The index of the selected option of a list. */
  const chosen = (name: string) =>
    browser.execute(
      (label) => [...document.querySelectorAll(`[role="listbox"][aria-label="${label}"] [role="option"]`)].findIndex((option) => option.getAttribute("aria-selected") === "true"),
      name,
    );
  const crumbs = () => browser.$('nav[aria-label="Breadcrumb"]').getText();

  before(async () => {
    checkBuild();
    const fixture = await create();
    // A second Run, so the Runs pane has two rows to move between.
    await verifyWaiting(fixture);
    session = await start(fixture, "--repo", fixture.repo);
    browser = session.browser;
    await eventually(async () => /Runs \(2\)/i.test(await browser.execute(() => document.body.innerText)), "two Runs");
    await browser.$('[role="tree"]').click();
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-MON-TREE-TO-RUNS E-MON-RUNS-MOVE E-MON-RUNS-TO-SCOPE E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-TREE
  it("steps out from the tree to the Runs and Scope and back in, moving the selection on the way", async () => {
    await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
    const run = (await chosen("Runs")) as number;
    const before = await crumbs();
    await press(browser, ["ArrowDown"], async () => (await chosen("Runs")) !== run, "another Run selected");
    await eventually(async () => (await crumbs()) !== before, "the tree shows the other Run");
    await press(browser, ["ArrowUp"], async () => (await chosen("Runs")) === run, "the first Run selected again");
    await press(browser, ["ArrowLeft"], () => listbox("Scope").isFocused(), "the Scope focused");
    const scope = (await chosen("Scope")) as number;
    await press(browser, ["ArrowUp"], async () => (await chosen("Scope")) === scope - 1, "the row above highlighted");
    await press(browser, ["ArrowUp"], async () => (await chosen("Scope")) === 0, "the ALL scope selected");
    await eventually(async () => (await crumbs()).includes("all repositories"), "the Runs of all repositories");
    await press(browser, ["Enter"], () => listbox("Runs").isFocused(), "the Runs focused again");
    await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the tree focused again");
  });
});

describe("quitting without a claim", () => {
  let session: Session;

  before(async () => {
    checkBuild();
    session = await start(await create());
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

describe("closing the window without a claim", () => {
  let session: Session;

  before(async () => {
    checkBuild();
    session = await start(await create());
    await session.browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-QUIT-CLEAN
  it("closes at once, asking nothing", async () => {
    const browser = session.browser;
    await browser.closeWindow().catch(() => {});
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
