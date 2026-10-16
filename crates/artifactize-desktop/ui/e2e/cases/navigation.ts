// Moving through the Monitor in the real app: the keys help, a refresh, folding the Run tree,
// jumping to what needs attention and to an upstream blocker, and stepping back out of a
// Detail; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { verifyWaiting, create, createNavigationFixture, createStates, requests } from "../fixture.ts";
import { git } from "../environment.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { sessionEndedByApp } from "../observation.ts";
import { nativePath } from "../platform.ts";
type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture">;

function monitorNavigation(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);

  const selected = () => browser.$('[role="treeitem"][aria-selected="true"]').getText();

  /** The tree row of an Artifact (level 1) or of an eval (level 2) by its name. */
  const row = (level: 1 | 2, name: string) =>
    browser.$(`//li[@role="treeitem"][@aria-level="${level}"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "${name}")]]`);

  const helpOpen = () => browser.execute(() => !!document.querySelector('[role="dialog"]'));
  return { text, selected, row, helpOpen };
}

function detailNavigation(browser: WebdriverIO.Browser) {
  /** The Detail pane's title: the Monitor's fourth pane (Scope, Runs, Run, Detail). */
  const detail = () =>
    browser.execute(() => document.querySelectorAll("section.panel h2")[3]?.textContent?.trim() ?? "");

  const row = (level: 1 | 2, name: string) =>
    browser.$(`//li[@role="treeitem"][@aria-level="${level}"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "${name}")]]`);
  return { detail, row };
}

function paneNavigation(browser: WebdriverIO.Browser) {
  const listbox = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);

  const runIds = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')]
    .map((row) => /run-\S+/.exec(row.innerText)?.[0] ?? "").sort());

  const selectedRun = () => browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");

  const chosen = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')]
    .findIndex((option) => option.getAttribute("aria-selected") === "true"));

  /** Assert the historical criterion in the actual Tree, unfolding a finished Artifact
   * through the UI if needed. Current declarations cannot substitute for the selected Run. */
  async function treeShows(run: { id: string; eval: string }) {
    const artifact = await browser.$('[role="treeitem"][data-path="a:subject"]');
    await artifact.waitForExist();
    await artifact.click();
    await press(browser, ["l"], async () => (await artifact.getAttribute("aria-expanded")) === "true", "the historical evals unfolded");
    await browser.waitUntil(async () => (await tree()).evals.includes(run.eval), { timeout: 15000, timeoutMsg: `${run.id}'s criterion is in the Tree` });
    assert.deepEqual((await tree()).evals, [run.eval], "the other Run's criterion is absent");
  }
  /** The Run pane's title, which names the Run its Tree shows. */
  const runTitle = () => browser.execute(() => [...document.querySelectorAll("section.panel h2")]
    .map((title) => title.textContent?.trim() ?? "").find((title) => title.startsWith("Run ")) ?? "");

  /** The Tree's current Artifact and its eval rows, read without moving the focus. */
  const tree = () => browser.execute(() => ({
    artifact: document.querySelector('[role="treeitem"][data-path="a:subject"]')?.getAttribute("aria-expanded") ?? null,
    evals: [...document.querySelectorAll('[role="treeitem"][aria-level="2"] .name')].map((name) => name.firstChild?.textContent?.trim() ?? ""),
  }));

  /** Moving in Runs replaces the Tree with the newly selected Run while Runs keeps the focus. */
  async function treeFollows(run: { id: string; eval: string }) {
    const shown = await tree();
    assert.equal(await runTitle(), `Run ${run.id}`, "the Run pane shows the selected Run");
    assert.notEqual(shown.artifact, null, "the selected Run's Artifact is in the Tree");
    assert.deepEqual(shown.evals, shown.artifact === "true" ? [run.eval] : [], "only the selected Run's criterion is in the Tree");
    assert.ok(await listbox("Runs").isFocused(), "Runs keeps the focus while the Tree follows");
  }
  return { listbox, runIds, selectedRun, chosen, treeShows, runTitle, treeFollows };
}

export const scenarios: Scenario[] = [
  // UX edges: E-HELP-OPEN E-HELP-CLOSE
  scenario<Record<string, never>>(
    "navigation-help",
    "lists the keys of the screen with ? and closes the list with Esc",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, helpOpen } = monitorNavigation(browser);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$('[role="tree"]').click();
      await press(browser, ["?"], helpOpen, "the keys dialog");
      const dialog = browser.$('[role="dialog"]');
      assert.match(await dialog.getText(), /Keyboard shortcuts/);
      const shortcuts = await browser.execute(() => [...document.querySelectorAll('[role="dialog"] dt')].map((key) => ({
        keys: key.textContent?.trim() ?? "",
        action: key.nextElementSibling?.textContent?.trim() ?? "",
      })));
      const expected: [RegExp, RegExp][] = [
        [/(?:^|[\s/])b(?:$|[\s/])/, /Upstream blocker/],
        [/h\s*\/\s*l/, /Fold\s*\/\s*unfold/],
        [/^!$/, /Next ERROR, RED or waiting Human eval/],
        [/^v$/, /Switch.*tree and graph/],
        [/\bF5\b/, /^Refresh$/],
      ];
      for (const [keys, action] of expected) {
        assert.ok(shortcuts.some((entry) => keys.test(entry.keys) && action.test(entry.action)),
          `the Monitor Help pairs ${keys} with ${action}: ${JSON.stringify(shortcuts)}`);
      }
      await press(browser, ["Escape"], async () => !(await helpOpen()), "the keys dialog closed");
    },
  ),

  // UX edges: E-MON-LIVE
  scenario<Record<string, never>>(
    "navigation-live-run",
    "shows an identified Run from another process without refreshing",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { text } = monitorNavigation(browser);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const rows = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')].map((row) => row.innerText));
      const before = await rows();
      const id = await verifyWaiting(fixture);
      assert.ok(before.every((row) => !row.includes(id)), "the new Run was not already listed");
      await browser.waitUntil(async () => (await rows()).some((row) => row.includes(id)), { timeout: 15000, timeoutMsg: "the subscribed new Run row appears without F5" });
    },
  ),

  // UX edges: E-REFRESH
  scenario<{ before: string; after: string; worktree: string }>(
    "navigation-refresh",
    "reloads external Git worktree metadata with F5 on the same Monitor screen",
    async () => {
      const fixture = await create();
      git(fixture.repo, "-C", fixture.repo, "add", ".");
      git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Refresh fixture inputs");
      const before = "before-refresh";
      const after = "after-refresh";
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", before, nativePath(fixture.root, before));
      return { fixture, args: ["--repo", fixture.repo], data: { before, after, worktree: nativePath(fixture.root, after) } };
    },
    async (browser, { fixture, data }) => {
      const { text } = monitorNavigation(browser);
      const { runIds } = paneNavigation(browser);
      const labels = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"] .label')]
        .map((label) => label.textContent?.trim() ?? ""));
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const previousRuns = await runIds();
      assert.ok(previousRuns.length > 0, "the fixture's saved Run is visible before the Git-only change");
      // Open the existing without-Runs group first, so F5 itself must reveal the
      // new worktree's actual identity, not merely change a collapsed group's count.
      await browser.$('[aria-label="Scope"] [role="option"].fold').click();
      await browser.waitUntil(async () => (await labels()).includes(data.before), { timeout: 15000, timeoutMsg: "the existing empty worktree is shown" });
      assert.equal((await labels()).includes(data.after), false, "the new worktree does not exist in the initial Scope view");
      // This is only an external Git input: no verify, state write or subscription hint.
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", data.after, data.worktree);
      assert.equal((await labels()).includes(data.after), false, "the open Monitor has not reloaded the newly created worktree");
      await press(browser, ["F5"], async () => (await labels()).includes(data.after), "F5 shows the new Git worktree in Scope", { handled: true });
      const refreshed = await labels();
      assert.ok(refreshed.includes(data.before), "refresh preserves the expanded group and its existing worktree");
      assert.ok(refreshed.includes(data.after), "refresh displays the exact worktree created by this case");
      assert.deepEqual(await runIds(), previousRuns, "the Git-only change created no Run and keeps the selected scope's saved Runs");
      assert.equal(await browser.$('nav[aria-label="Breadcrumb"] [role="status"]').isExisting(), false, "the refresh completed without a native read error");
      assert.equal(await browser.$('[role="tab"][data-state="active"]').getText(), "Monitor", "on the same screen");
    },
  ),

  // UX edges: E-MON-TREE-FOLD
  scenario<Record<string, never>>(
    "navigation-fold",
    "folds and unfolds an Artifact's evals with h, l and Space",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, row } = monitorNavigation(browser);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      const release = await row(1, "release");
      await browser.$('[role="tree"]').click();
      await release.click();
      const expanded = async () => (await row(1, "release")).getAttribute("aria-expanded");
      // The release Artifact's one eval row is what folding hides and unfolding restores.
      const evalShown = async () => (await row(2, "signoff")).isDisplayed();
      const folded = async () => (await expanded()) === "false" && !(await evalShown());
      const unfolded = async () => (await expanded()) === "true" && await evalShown();
      assert.ok(await unfolded(), "release starts unfolded with its eval row");
      await press(browser, ["h"], folded, "release and its eval row folded by h");
      await press(browser, ["l"], unfolded, "release and its eval row unfolded by l");
      await press(browser, [" "], folded, "release and its eval row folded by Space");
      await press(browser, [" "], unfolded, "release and its eval row unfolded by Space");
    },
  ),

  // UX edges: E-MON-BLOCKER
  scenario<Record<string, never>>(
    "navigation-blocker",
    "jumps from a blocked eval to the upstream Artifact it waits for with b",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { text, selected, row } = monitorNavigation(browser);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$('[role="tree"]').click();
      await (await row(2, "check")).click();
      const level = () => browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("aria-level");
      await press(
        browser,
        ["b"],
        async () => (await level()) === "1" && /release/.test(await selected()),
        "the upstream Artifact selected",
      );
    },
  ),

  // UX edges: E-MON-DETAIL-TECHNICAL E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "navigation-technical-detail",
    "shows the technical record of an opened Detail with t and steps back to the tree",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { text, row } = monitorNavigation(browser);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$('[role="tree"]').click();
      await (await row(2, "check")).click();
      const close = () => browser.$("button*=Close");
      await press(browser, ["Enter"], () => close().isExisting(), "the Detail opened");
      const request = requests(fixture).find((saved) => saved.evalId === "ship/check");
      assert.ok(request, "the fixture independently identifies ship/check");
      const field = (name: string) => browser.execute((key) => [...document.querySelectorAll('[role="document"] dt')]
        .find((entry) => entry.textContent === key)?.nextElementSibling?.textContent ?? "", name);
      await browser.waitUntil(async () => (await field("Request")) === request.id, { timeout: 15000, timeoutMsg: "the opened Detail identifies ship/check by default" });
      const technical = () => browser.$("button*=Technical").getAttribute("aria-expanded");
      assert.equal(await technical(), "false");
      assert.equal(await field("Fingerprint"), "");
      await press(browser, ["t"], async () => (await technical()) === "true" && (await field("Fingerprint")) !== "", "the actual additional technical record shown");
      await press(browser, ["t"], async () => (await technical()) === "false" && (await field("Fingerprint")) === "", "the additional technical record hidden");
      assert.equal(await field("Request"), request.id, "hiding Technical does not hide the default request");
      await press(browser, ["Escape"], async () => !(await close().isExisting()), "the Detail closed");
      await browser.waitUntil(() => browser.$('[role="tree"]').isFocused(), { timeout: 15000, timeoutMsg: "focus back on the tree" });
    },
  ),

  // UX edges: E-MON-TREE-MOVE E-MON-ATTENTION
  scenario<Record<string, never>>(
    "navigation-attention-order",
    "visits the next ERROR, RED and waiting Human eval in tree order, then wraps",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
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
    },
  ),

  // UX edges: E-MON-TREE-MOVE E-MON-TREE-DETAIL
  scenario<Record<string, never>>(
    "navigation-detail-after-enter",
    "shows the newly selected row after the Detail was opened with Enter",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { detail, row } = detailNavigation(browser);
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$('[role="tree"]').click();
      await (await row(2, "check")).click();
      await browser.waitUntil(async () => /check/.test(await detail()), { timeout: 15000, timeoutMsg: "the peek of ship/check" });
      const close = () => browser.$("button*=Close");
      await press(browser, ["Enter"], () => close().isExisting(), "the Detail opened");
      // Return to the focusable tree pane, then select another row with the pointer.
      await browser.$('[role="tree"]').click();
      await (await row(1, "docs")).click();
      await browser.waitUntil(async () => /docs/.test(await detail()), { timeout: 15000, timeoutMsg: "the Detail shows docs after a click" });
      // ... and the selection moves by key.
      await press(browser, ["k"], async () => !/docs/.test(await detail()), "the Detail leaves docs after k");
      await press(browser, ["j"], async () => /docs/.test(await detail()), "the Detail returns to docs after j");
    },
  ),

  // UX edges: E-MON-TREE-MOVE E-MON-TREE-DETAIL
  scenario<Record<string, never>>(
    "navigation-detail-after-click",
    "shows the newly selected row after a click into the Detail",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { detail, row } = detailNavigation(browser);
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      await browser.$('[role="tree"]').click();
      await (await row(2, "check")).click();
      await browser.waitUntil(async () => /check/.test(await detail()), { timeout: 15000, timeoutMsg: "the Detail shows ship/check" });
      const panes = await browser.$$("section.panel");
      await panes[3].click();
      await (await row(1, "docs")).click();
      await browser.waitUntil(async () => /docs/.test(await detail()), { timeout: 15000, timeoutMsg: "the Detail shows docs after a click" });
      await browser.$('[role="tree"]').click();
      await (await row(2, "check")).click();
      await browser.waitUntil(async () => /check/.test(await detail()), { timeout: 15000, timeoutMsg: "the Detail follows back to ship/check" });
    },
  ),

  // UX edges: E-MON-TREE-TO-RUNS E-MON-RUNS-MOVE E-MON-RUNS-TREE
  scenario<NavigationData>(
    "navigation-run-history",
    "switches identified Runs with the keyboard and replaces the Tree with each saved criterion",
    async () => {
      const example = await createNavigationFixture();
      const fixture = example.fixture;
      return { fixture, args: ["--repo", fixture.repo], data: { scopes: example.scopes, runs: example.runs } };
    },
    async (browser, { data: example }) => {
      const { listbox, runIds, selectedRun, treeShows, runTitle, treeFollows } = paneNavigation(browser);
      await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the repository's exact Runs" });
      await browser.$('[role="tree"]').click();
      const [older, newer] = example.runs;
      assert.equal(await selectedRun(), newer.id, "the explicitly newer fixture Run is selected first");
      await treeShows(newer);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
      await press(browser, ["ArrowDown"], async () => (await selectedRun()) === older.id && (await runTitle()) === `Run ${older.id}`, "the older identified Run selected and shown");
      await treeFollows(older);
      await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the older Run's Tree focused");
      await treeShows(older);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "back to Runs");
      await press(browser, ["ArrowUp"], async () => (await selectedRun()) === newer.id && (await runTitle()) === `Run ${newer.id}`, "the newer identified Run selected and shown again");
      await treeFollows(newer);
      await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the newer Run's Tree focused");
      await treeShows(newer);
    },
  ),

  // UX edges: E-MON-TREE-TO-RUNS E-MON-RUNS-TO-SCOPE E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-TREE
  scenario<NavigationData>(
    "navigation-repository-scopes",
    "narrows to different repositories and ALL with keyboard scopes and lists their exact Run IDs",
    async () => {
      const example = await createNavigationFixture();
      const fixture = example.fixture;
      return { fixture, args: ["--repo", fixture.repo], data: { scopes: example.scopes, runs: example.runs } };
    },
    async (browser, { data: example }) => {
      const { listbox, runIds, chosen } = paneNavigation(browser);
      await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the repository's exact Runs" });
      await browser.$('[role="tree"]').click();
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
        await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...ids].sort()), { timeout: 15000, timeoutMsg: `only ${label}'s exact Runs` });
        assert.deepEqual(await runIds(), [...ids].sort());
      };
      await select("ALL", [...example.scopes.repo, ...example.scopes.other]);
      await select("other", example.scopes.other);
      await select("repo", example.scopes.repo);
      await press(browser, ["Enter"], () => listbox("Runs").isFocused(), "the narrowed Runs focused");
      await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "the narrowed Tree focused");
    },
  ),

  // UX edges: E-QUIT-CLEAN
  scenario<Record<string, never>>(
    "navigation-quit-clean",
    "closes the window at once with q",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
      await browser.$('[role="tree"]').click();
      // Observe the real window's exit before retiring the ended WebDriver client session.
      await sessionEndedByApp(browser, () => browser.keys(["q"]));
    },
  ),

  // UX edges: E-QUIT-CLEAN
  scenario<Record<string, never>>(
    "navigation-close-clean",
    "closes at once, asking nothing",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await browser.$('[role="tree"]').waitForExist({ timeout: 15000 });
      await sessionEndedByApp(browser, () => browser.closeWindow());
    },
  ),
];
