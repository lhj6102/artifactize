// Moving through the Monitor in the real app: the keys help, a refresh, folding the Run tree,
// jumping to what needs attention and to an upstream blocker, and stepping back out of a
// Detail; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { verifyWaiting, create, createNavigationFixture, createStates, type Fixture, requests } from "./fixture.ts";
import { checkBuild, type Session, start } from "./driver.ts";
import { press } from "./keys.ts";
import { checkpoint, commandsRendered } from "./commands.ts";

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
    const rows = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')].map((row) => row.innerText));
    const previous = await rows();
    const id = await verifyWaiting(fixture);
    assert.ok(previous.every((row) => !row.includes(id)), "this Run is a new disk change");
    // A subscription may already have shown the Run. That alone cannot prove F5 worked:
    // require its consumed key and a new completed native browse, not a dispatch counter.
    await commandsRendered(browser);
    const before = (await checkpoint(browser)).activity.commands.browse?.completed ?? 0;
    await press(browser, ["F5"], async () => {
      const browse = (await checkpoint(browser)).activity.commands.browse;
      return !!browse && browse.completed > before && browse.pending === 0;
    }, "the refresh's native read completed", { handled: true });
    await commandsRendered(browser);
    await eventually(async () => (await rows()).some((row) => row.includes(id)), "the exact refreshed Run rendered");
    assert.equal((await rows()).length, previous.length + 1, "the refreshed list contains this case's newly submitted Run");
    assert.equal(await browser.$('[role="tab"][data-state="active"]').getText(), "Monitor", "on the same screen");
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

describe("attention across a Run's verdicts", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;

  beforeEach(async () => {
    checkBuild();
    fixture = await createStates();
    session = await start(fixture);
    browser = session.browser;
    await browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
  });

  afterEach(async () => {
    await session?.dispose();
  });

  // UX edges: E-MON-TREE-MOVE E-MON-ATTENTION
  it("visits the next ERROR, RED and waiting Human eval in tree order, then wraps", async () => {
    const eligible = new Map([
      ["crash", { eval: "check", status: "ERROR" }],
      ["red", { eval: "check", status: "RED" }],
      ["gate", { eval: "signoff", status: "WAITING_HUMAN" }],
    ]);
    const saved = requests(fixture);
    for (const [artifact, expected] of eligible) {
      assert.equal(saved.find((request) => request.evalId === `${artifact}/${expected.eval}`)?.status, expected.status,
        `${artifact} actually reached its declared attention state`);
    }
    // The UX says next in the shown tree, without prescribing a topological tie-break.
    // Read its Artifact order, but use fixture-owned statuses, not the app's urgent flags.
    const order = (await browser.execute(() => [...document.querySelectorAll('[role="treeitem"][aria-level="1"]')]
      .map((row) => row.getAttribute("data-path")?.slice(2) ?? ""))).filter((name) => eligible.has(name));
    assert.deepEqual([...order].sort(), [...eligible.keys()].sort());
    await browser.$(`[role="treeitem"][data-path="a:${order[0]}"]`).click();
    for (const artifact of [...order, order[0]]) {
      const expected = eligible.get(artifact)!;
      const path = `a:${artifact}/e:${artifact}/${expected.eval}`;
      await press(browser, ["!"], async () =>
        (await browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("data-path")) === path,
      `the next eligible eval is ${artifact}/${expected.eval} (${expected.status})`);
      assert.equal(await browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("aria-level"), "2", "an eval, not its Artifact");
    }
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

  beforeEach(async () => {
    checkBuild();
    session = await start(await create());
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), "the Run tree");
  });

  afterEach(async () => {
    await session?.dispose();
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
    const panes = await browser.$$("section.panel");
    await panes[3].click();
    await (await row(1, "docs")).click();
    await eventually(async () => /docs/.test(await detail()), "the Detail shows docs after a click");
    await (await row(2, "check")).click();
    await eventually(async () => /check/.test(await detail()), "the Detail follows back to ship/check");
  });
});

describe("moving between the panes", () => {
  let example: Awaited<ReturnType<typeof createNavigationFixture>>;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const listbox = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);
  const runIds = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')]
    .map((row) => /run-\S+/.exec(row.innerText)?.[0] ?? "").sort());
  const selectedRun = () => browser.$('[aria-label="Runs"] [aria-selected="true"]').getText();
  const chosen = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')]
    .findIndex((option) => option.getAttribute("aria-selected") === "true"));

  beforeEach(async () => {
    checkBuild();
    example = await createNavigationFixture();
    session = await start(example.fixture, "--repo", example.fixture.repo);
    browser = session.browser;
    await eventually(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), "the repository's exact Runs");
    await browser.$('[role="tree"]').click();
  });

  afterEach(async () => {
    await session?.dispose();
  });

  /** Assert the historical criterion in the actual Tree, unfolding a finished Artifact
   * through the UI if needed. Current declarations cannot substitute for the selected Run. */
  async function treeShows(run: { id: string; eval: string }) {
    const artifact = await browser.$('[role="treeitem"][data-path="a:subject"]');
    await artifact.waitForExist();
    await artifact.click();
    await press(browser, ["l"], async () => (await artifact.getAttribute("aria-expanded")) === "true", "the historical evals unfolded");
    const names = () => browser.execute(() => [...document.querySelectorAll('[role="treeitem"][aria-level="2"] .name')]
      .map((name) => name.firstChild?.textContent?.trim() ?? ""));
    await eventually(async () => (await names()).includes(run.eval), `${run.id}'s criterion is in the Tree`);
    assert.deepEqual(await names(), [run.eval], "the other Run's criterion is absent");
  }

  // UX edges: E-MON-TREE-TO-RUNS E-MON-RUNS-MOVE E-MON-RUNS-TREE
  it("switches identified Runs with the keyboard and replaces the Tree with each saved criterion", async () => {
    const [older, newer] = example.runs;
    assert.ok((await selectedRun()).includes(newer.id), "newest first");
    await treeShows(newer);
    await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
    await press(browser, ["ArrowDown"], async () => (await selectedRun()).includes(older.id), "the older identified Run selected");
    await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the older Run's Tree focused");
    await treeShows(older);
    await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "back to Runs");
    await press(browser, ["ArrowUp"], async () => (await selectedRun()).includes(newer.id), "the newer identified Run selected again");
    await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the newer Run's Tree focused");
    await treeShows(newer);
  });

  // UX edges: E-MON-TREE-TO-RUNS E-MON-RUNS-TO-SCOPE E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-TREE
  it("narrows to different repositories and ALL with keyboard scopes and lists their exact Run IDs", async () => {
    await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
    await press(browser, ["ArrowLeft"], () => listbox("Scope").isFocused(), "the Scope focused");
    const select = async (label: string, ids: string[]) => {
      const labels = await browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"] .label')]
        .map((row) => row.textContent?.trim() ?? ""));
      const target = labels.indexOf(label);
      assert.ok(target >= 0, `the ${label} scope exists`);
      while ((await chosen()) !== target) {
        const before = await chosen();
        const direction = before < target ? 1 : -1;
        await press(browser, [direction === 1 ? "ArrowDown" : "ArrowUp"], async () => (await chosen()) === before + direction, `keyboard scope step toward ${label}`);
      }
      await eventually(async () => JSON.stringify(await runIds()) === JSON.stringify([...ids].sort()), `only ${label}'s exact Runs`);
      assert.deepEqual(await runIds(), [...ids].sort());
    };
    await select("ALL", [...example.scopes.repo, ...example.scopes.other]);
    await select("other", example.scopes.other);
    await select("repo", example.scopes.repo);
    await press(browser, ["Enter"], () => listbox("Runs").isFocused(), "the narrowed Runs focused");
    await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the narrowed Tree focused");
  });
});

describe("quitting without a claim", () => {
  let session: Session;

  beforeEach(async () => {
    checkBuild();
    session = await start(await create());
    await session.browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
  });

  afterEach(async () => {
    await session?.dispose();
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

  beforeEach(async () => {
    checkBuild();
    session = await start(await create());
    await session.browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
  });

  afterEach(async () => {
    await session?.dispose();
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
