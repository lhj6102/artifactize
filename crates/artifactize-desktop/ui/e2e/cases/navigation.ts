// Moving through the Monitor in the real app: the keys help, a refresh, the summary under the
// pointer, jumping to what needs attention and to an upstream blocker, and stepping back out of
// a Detail; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { verifyWaiting, create, createNavigationFixture, createStates, createUnreadableRuns, requests } from "../fixture.ts";
import { git } from "../environment.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { sessionEndedByApp } from "../observation.ts";
import { nativePath, requestWindowClose } from "../platform.ts";
import { focusedNode, graphShown, nodeAt, popup, rowAt, summaryShows, tracePointer } from "../graph-view.ts";
type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture">;

function monitorNavigation(browser: WebdriverIO.Browser) {
  const helpOpen = () => browser.execute(() => !!document.querySelector('[role="dialog"]'));
  const runsFocused = () => browser.$('[role="listbox"][aria-label="Runs"]').isFocused();
  return { helpOpen, runsFocused };
}

function paneNavigation(browser: WebdriverIO.Browser) {
  const listbox = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);

  const runIds = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')]
    .map((row) => /run-\S+/.exec(row.innerText)?.[0] ?? "").sort());

  const selectedRun = () => browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");

  const chosen = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')]
    .findIndex((option) => option.getAttribute("aria-selected") === "true"));

  /** The Run pane's title, which names the Run its graph shows. */
  const runTitle = () => browser.execute(() => [...document.querySelectorAll("section.panel h2")]
    .map((title) => title.textContent?.trim() ?? "").find((title) => title.startsWith("Run ")) ?? "");

  /** From the focused graph, pop up the subject Artifact's evals: the selected Run's own
   * historical criterion, and no other. Current declarations cannot substitute for it. */
  async function graphShows(run: { id: string; eval: string }) {
    await press(browser, ["Enter"], async () => (await popup(browser))?.focused === true, `${run.id}'s subject evals popped up`);
    const shown = await popup(browser);
    assert.equal(shown?.artifact, "subject");
    assert.deepEqual(shown?.evals, [run.eval], "only the selected Run's criterion is in its popup");
    await press(browser, ["Escape"], async () => (await popup(browser)) === null && (await focusedNode(browser)) === "a:subject", "the popup closed on subject");
  }

  /** Moving in Runs replaces the graph with the newly selected Run while Runs keeps the focus. */
  async function graphFollows(run: { id: string; eval: string }) {
    assert.equal(await runTitle(), `Run ${run.id}`, "the Run pane shows the selected Run");
    await graphShown(browser, "a:subject");
    assert.ok(await listbox("Runs").isFocused(), "Runs keeps the focus while the graph follows");
  }
  return { listbox, runIds, selectedRun, chosen, graphShows, runTitle, graphFollows };
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
      const { helpOpen, runsFocused } = monitorNavigation(browser);
      await graphShown(browser, "a:release");
      await browser.waitUntil(runsFocused, { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      await press(browser, ["?"], helpOpen, "the keys dialog");
      const dialog = browser.$('[role="dialog"]');
      assert.match(await dialog.getText(), /Keyboard shortcuts/);
      const shortcuts = await browser.execute(() => [...document.querySelectorAll('[role="dialog"] dt')].map((key) => ({
        keys: key.textContent?.trim() ?? "",
        action: key.nextElementSibling?.textContent?.trim() ?? "",
      })));
      const expected: [RegExp, RegExp][] = [
        [/(?:^|[\s/])b(?:$|[\s/])/, /Upstream blocker/],
        [/^!$/, /Next ERROR, RED or waiting Human eval/],
        [/^Enter \/ click$/, /Pop up the Artifact's evals/],
        [/^o$/, /Open the Detail of the selected Artifact or eval/],
        [/\bF5\b/, /^Refresh$/],
      ];
      for (const [keys, action] of expected) {
        assert.ok(shortcuts.some((entry) => keys.test(entry.keys) && action.test(entry.action)),
          `the Monitor Help pairs ${keys} with ${action}: ${JSON.stringify(shortcuts)}`);
      }
      assert.ok(!shortcuts.some((entry) => /^v$/.test(entry.keys)), "no key switches to a tree");
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
      await graphShown(browser, "a:release");
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
      const { runIds } = paneNavigation(browser);
      const labels = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"] .label')]
        .map((label) => label.textContent?.trim() ?? ""));
      await graphShown(browser, "a:release");
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
      assert.equal(await browser.$('.monitor [role="status"]').isExisting(), false, "the refresh completed without a native read error");
      assert.equal(await browser.$('[role="tab"][data-state="active"]').getText(), "Monitor", "on the same screen");
    },
  ),

  // UX edges: E-MON-RUNS-GRAPH E-GRAPH-MOVE E-MON-BLOCKER
  scenario<Record<string, never>>(
    "navigation-blocker",
    "jumps from an Artifact waiting for another to that upstream Artifact with b",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      const { runsFocused } = monitorNavigation(browser);
      await graphShown(browser, "a:ship");
      await browser.waitUntil(runsFocused, { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      // The Run starts on the release sign-off; its Artifact takes the keyboard in the graph.
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:release", "release focused in the graph");
      await press(browser, ["ArrowRight"], async () => (await focusedNode(browser)) === "a:ship", "ship, which waits for release, focused");
      await summaryShows(browser, nodeAt("a:ship"), /waits for release/);
      await press(browser, ["b"], async () => (await focusedNode(browser)) === "a:release", "release, the upstream Artifact ship waits for, focused");
      await summaryShows(browser, nodeAt("a:release"), /release/);
    },
  ),

  // UX edges: E-GRAPH-EVALS-OPEN E-GRAPH-EVALS-DETAIL E-MON-DETAIL-TECHNICAL E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "navigation-technical-detail",
    "shows the technical record of an opened Detail with t and steps back to the graph as it was",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:ship");
      await browser.$(nodeAt("a:ship")).click();
      await browser.waitUntil(async () => (await popup(browser))?.focused === true, { timeout: 15000, timeoutMsg: "ship's evals popped up" });
      assert.equal((await popup(browser))?.selected, "a:ship/e:ship/check");
      const detail = () => browser.$('aside[aria-label="Detail"]');
      await press(browser, ["Enter"], () => detail().isExisting(), "the Detail opened");
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
      await press(browser, ["Escape"], async () => !(await detail().isExisting()), "the Detail closed");
      // The graph as it was: ship's popup, check selected, its list taking the keys again.
      await browser.waitUntil(async () => {
        const shown = await popup(browser);
        return shown?.artifact === "ship" && shown.selected === "a:ship/e:ship/check" && shown.focused;
      }, { timeout: 15000, timeoutMsg: "focus back in ship's popup on check" });
    },
  ),

  // UX edges: E-MON-ATTENTION
  scenario<Record<string, never>>(
    "navigation-attention-order",
    "pops up each ERROR, RED and waiting Human eval in its Artifact's evals in turn, then wraps",
    async () => {
      const fixture = await createStates();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:crash");
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
      // The UX names no order: each press shows another eligible eval in its Artifact's
      // popup, all of them in as many presses, and the next press starts over.
      const visited: string[] = [];
      for (let round = 0; round <= eligible.size; round += 1) {
        const before = (await popup(browser))?.selected ?? null;
        await press(browser, ["!"], async () => {
          const shown = await popup(browser);
          return !!shown?.focused && shown.selected !== null && shown.selected !== before;
        }, "the next eligible eval in its Artifact's popup");
        const shown = (await popup(browser))!;
        const expected = eligible.get(shown.artifact);
        assert.ok(expected, `${shown.artifact} is one of the eligible Artifacts`);
        assert.equal(shown.selected, `a:${shown.artifact}/e:${shown.artifact}/${expected.eval}`, `its ${expected.status} eval selected`);
        visited.push(shown.artifact);
      }
      assert.deepEqual(visited.slice(0, eligible.size).sort(), [...eligible.keys()].sort(), "every eligible eval once");
      assert.equal(visited[eligible.size], visited[0], "then the first again");
    },
  ),

  // UX edges: E-MON-SUMMARY
  scenario<Record<string, never>>(
    "navigation-summary-hover",
    "shows the summary of the Artifact or eval under the pointer beside it",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await tracePointer(browser);
      await graphShown(browser, "a:docs");
      await browser.$(nodeAt("a:docs")).moveTo();
      await summaryShows(browser, nodeAt("a:docs"), /docs/);
      await graphShown(browser, "a:ship");
      await browser.$(nodeAt("a:ship")).moveTo();
      await summaryShows(browser, nodeAt("a:ship"), /waits for release/);
      // An eval of a popup under the pointer: its own summary beside its row.
      await browser.$(nodeAt("a:release")).click();
      await browser.waitUntil(async () => (await popup(browser))?.artifact === "release", { timeout: 15000, timeoutMsg: "release's evals popped up" });
      await browser.$(rowAt("a:release/e:release/signoff")).moveTo();
      await summaryShows(browser, rowAt("a:release/e:release/signoff"), /WAITING_HUMAN/);
    },
  ),

  // UX edges: E-MON-GRAPH-TO-RUNS E-MON-RUNS-MOVE E-MON-RUNS-GRAPH
  scenario<NavigationData>(
    "navigation-run-history",
    "switches identified Runs with the keyboard and pops up each Run's saved criterion",
    async () => {
      const example = await createNavigationFixture();
      const fixture = example.fixture;
      return { fixture, args: ["--repo", fixture.repo], data: { scopes: example.scopes, runs: example.runs } };
    },
    async (browser, { data: example }) => {
      const { listbox, runIds, selectedRun, graphShows, runTitle, graphFollows } = paneNavigation(browser);
      await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the repository's exact Runs" });
      await browser.waitUntil(() => listbox("Runs").isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      const [older, newer] = example.runs;
      assert.equal(await selectedRun(), newer.id, "the explicitly newer fixture Run is selected first");
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:subject", "the newer Run's graph focused");
      await graphShows(newer);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
      await press(browser, ["ArrowDown"], async () => (await selectedRun()) === older.id && (await runTitle()) === `Run ${older.id}`, "the older identified Run selected and shown");
      await graphFollows(older);
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:subject", "the older Run's graph focused");
      await graphShows(older);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "back to Runs");
      await press(browser, ["ArrowUp"], async () => (await selectedRun()) === newer.id && (await runTitle()) === `Run ${newer.id}`, "the newer identified Run selected and shown again");
      await graphFollows(newer);
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:subject", "the newer Run's graph focused again");
      await graphShows(newer);
    },
  ),

  // UX journey J-MON-CATALOG: the selected Run fills the right side as its graph, also while
  // other Runs on the page cannot be read and the Run pane says so.
  scenario<{ readable: string; unreadable: string[] }>(
    "navigation-unreadable-runs",
    "shows the selected Run's graph within its pane while the pane names the Runs that cannot be read",
    async () => {
      const { fixture, readable, unreadable, restore } = createUnreadableRuns();
      return { fixture, args: ["--repo", fixture.repo], data: { readable, unreadable }, release: async () => restore() };
    },
    async (browser, { data: { readable, unreadable } }) => {
      const { runTitle } = paneNavigation(browser);
      await browser.waitUntil(async () => (await runTitle()) === `Run ${readable}`, { timeout: 15000, timeoutMsg: "the readable newest Run is shown" });
      const pane = () => browser.execute((title) => {
        const section = document.querySelector<HTMLElement>(`section.panel[aria-label="${title}"]`);
        const heading = section?.querySelector<HTMLElement>("h2");
        const notice = section?.querySelector<HTMLElement>('[role="status"]');
        const node = section?.querySelector<HTMLElement>('.svelte-flow__node[data-id="a:n000"]');
        if (!section || !heading || !notice || !node) return null;
        const box = section.getBoundingClientRect();
        const within = (element: HTMLElement) => {
          const inner = element.getBoundingClientRect();
          return inner.width > 0 && inner.left >= box.left && inner.right <= box.right;
        };
        return {
          // The pane stays inside the window, and what it shows stays inside the pane.
          inWindow: box.left >= 0 && box.right <= window.innerWidth,
          bodyWithin: [...section.children].every((child) => within(child as HTMLElement)),
          nodeWithin: within(node),
          noticeWithin: within(notice),
          titleWhole: within(heading) && heading.scrollWidth <= heading.clientWidth,
          tooltip: notice.title,
        };
      }, `Run ${readable}`);
      let last: Awaited<ReturnType<typeof pane>> = null;
      await browser.waitUntil(async () => {
        last = await pane();
        return !!last && last.inWindow && last.bodyWithin && last.nodeWithin && last.noticeWithin && last.titleWhole;
      }, { timeout: 15000, timeoutMsg: "the Run pane keeps its title, notice and graph within its width" })
        .catch((error: unknown) => { throw new Error(`${String(error)}; the pane last showed ${JSON.stringify(last)}`); });
      await graphShown(browser, "a:n000");
      const { tooltip } = (await pane())!;
      for (const id of unreadable) assert.match(tooltip, new RegExp(`Unreadable run ${id}: unknown variant \`dependency\``), `the notice names ${id} in full`);
      assert.doesNotMatch(tooltip, new RegExp(readable), "the readable Run is not named unreadable");
    },
  ),

  // UX edges: E-MON-GRAPH-TO-RUNS E-MON-RUNS-TO-SCOPE E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-GRAPH
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
      await browser.waitUntil(() => listbox("Runs").isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:subject", "the graph focused");
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
      await press(browser, ["Enter"], async () => (await focusedNode(browser)) === "a:subject", "the narrowed Run's graph focused");
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
      await graphShown(browser, "a:release");
      await browser.waitUntil(monitorNavigation(browser).runsFocused, { timeout: 15000, timeoutMsg: "the Runs pane focused" });
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
      await graphShown(browser, "a:release");
      await sessionEndedByApp(browser, () => requestWindowClose(browser));
    },
  ),
];
