// Moving through the Monitor in the real app: the keys help, a refresh, the pointer that only
// highlights, a Detail opened by a click and stepped back out of, unreadable Runs beside a
// readable one; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { verifyWaiting, create, createNavigationFixture, createUnreadableRuns, requests } from "../fixture.ts";
import { git } from "../environment.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { sessionEndedByApp } from "../observation.ts";
import { nativePath, requestWindowClose } from "../platform.ts";
import { detail, graphShown, nodeAt, openArtifact, openTab } from "../graph-view.ts";
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

  /** The graph's pane has the keyboard. */
  const graphFocused = () => browser.$('section.panel[aria-label^="Run run-"]').isFocused();

  /** A click on subject opens its Detail: the selected Run's own historical criterion, and no
   * other. Current declarations cannot substitute for it. Esc closes the Detail again. */
  async function graphShows(run: { id: string; eval: string }) {
    const shown = await openArtifact(browser, "a:subject", "Artifact subject");
    assert.deepEqual(shown.evals.map((row) => row.name), [run.eval], "only the selected Run's criterion is in subject's Detail");
    await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
  }

  /** Moving in Runs replaces the graph with the newly selected Run while Runs keeps the focus. */
  async function graphFollows(run: { id: string; eval: string }) {
    assert.equal(await runTitle(), `Run ${run.id}`, "the Run pane shows the selected Run");
    await graphShown(browser, "a:subject");
    assert.ok(await listbox("Runs").isFocused(), "Runs keeps the focus while the graph follows");
  }
  return { listbox, runIds, selectedRun, chosen, graphFocused, graphShows, runTitle, graphFollows };
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
        [/^Click an Artifact$/, /Open its Detail/],
        [/^Click the background$/, /Close the Detail/],
        [/^Click a tab or an eval$/, /Show the Artifact or that eval/],
        [/\bF5\b/, /^Refresh$/],
      ];
      for (const [keys, action] of expected) {
        assert.ok(shortcuts.some((entry) => keys.test(entry.keys) && action.test(entry.action)),
          `the Monitor Help pairs ${keys} with ${action}: ${JSON.stringify(shortcuts)}`);
      }
      assert.ok(!shortcuts.some((entry) => /^v$/.test(entry.keys)), "no key switches to a tree");
      assert.ok(!shortcuts.some((entry) => /^(?:!|o|b \/ Backspace|← \/ →)$/.test(entry.keys)), `the graph has no keys of its own: ${JSON.stringify(shortcuts)}`);
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

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "navigation-technical-detail",
    "shows the technical record of an eval's tab with t and closes the Detail back to the graph",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:ship");
      await openArtifact(browser, "a:ship", "Artifact ship");
      await openTab(browser, "e:ship/check");
      const request = requests(fixture).find((saved) => saved.evalId === "ship/check");
      assert.ok(request, "the fixture independently identifies ship/check");
      const field = (name: string) => browser.execute((key) => [...document.querySelectorAll('aside[aria-label="Detail"] [role="document"] dt')]
        .find((entry) => entry.textContent === key)?.nextElementSibling?.textContent ?? "", name);
      await browser.waitUntil(async () => (await field("Request")) === request.id, { timeout: 15000, timeoutMsg: "the opened Detail identifies ship/check by default" });
      const technical = () => browser.$('aside[aria-label="Detail"]').$("button*=Technical").getAttribute("aria-expanded");
      assert.equal(await technical(), "false");
      assert.equal(await field("Fingerprint"), "");
      await press(browser, ["t"], async () => (await technical()) === "true" && (await field("Fingerprint")) !== "", "the actual additional technical record shown");
      await press(browser, ["t"], async () => (await technical()) === "false" && (await field("Fingerprint")) === "", "the additional technical record hidden");
      assert.equal(await field("Request"), request.id, "hiding Technical does not hide the default request");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // The graph as it was, every Artifact still drawn and reachable.
      for (const id of ["a:release", "a:ship", "a:docs"]) await graphShown(browser, id);
    },
  ),

  // The pointer rule of the UX journeys: hovering opens nothing; information opens on a click.
  scenario<Record<string, never>>(
    "navigation-hover-opens-nothing",
    "opens nothing for the Artifact under the pointer, and opens its Detail on a click",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:ship");
      // Where the pointer's moves landed, logged before any handler without changing them.
      await browser.execute(() => {
        const log: string[] = [];
        (window as unknown as { overs: string[] }).overs = log;
        document.addEventListener("mousemove", (event) => {
          const node = (event.target as Element | null)?.closest?.(".svelte-flow__node");
          if (node) log.push(node.getAttribute("data-id") ?? "");
        }, true);
      });
      const look = () => browser.execute((selector) => ({
        over: (window as unknown as { overs: string[] }).overs.slice(),
        opened: !!document.querySelector('aside[aria-label="Detail"], [aria-label="Summary"], [aria-label^="Evals of "], [role="tooltip"]'),
        tooltip: !!document.querySelector(selector)?.querySelector("[title]") || !!document.querySelector(selector)?.closest("[title]"),
      }), nodeAt("a:ship"));
      await browser.$(nodeAt("a:ship")).moveTo();
      await browser.waitUntil(async () => (await look()).over.includes("a:ship"), { timeout: 15000, timeoutMsg: "the pointer moved over ship" });
      const hovered = await look();
      assert.equal(hovered.opened, false, "hovering opens no Detail, summary, eval list or tooltip");
      assert.equal(hovered.tooltip, false, "the Artifact carries no tooltip");
      // The same probe sees what a click opens.
      await browser.$(nodeAt("a:ship")).click();
      await browser.waitUntil(async () => (await look()).opened, { timeout: 15000, timeoutMsg: "a click opened ship's Detail" });
      assert.equal((await detail(browser))?.title, "Artifact ship");
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
      const { listbox, runIds, selectedRun, graphFocused, graphShows, runTitle, graphFollows } = paneNavigation(browser);
      await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the repository's exact Runs" });
      await browser.waitUntil(() => listbox("Runs").isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      const [older, newer] = example.runs;
      assert.equal(await selectedRun(), newer.id, "the explicitly newer fixture Run is selected first");
      await press(browser, ["Enter"], graphFocused, "the newer Run's graph focused");
      await graphShows(newer);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "the Runs focused");
      await press(browser, ["ArrowDown"], async () => (await selectedRun()) === older.id && (await runTitle()) === `Run ${older.id}`, "the older identified Run selected and shown");
      await graphFollows(older);
      await press(browser, ["Enter"], graphFocused, "the older Run's graph focused");
      await graphShows(older);
      await press(browser, ["Escape"], () => listbox("Runs").isFocused(), "back to Runs");
      await press(browser, ["ArrowUp"], async () => (await selectedRun()) === newer.id && (await runTitle()) === `Run ${newer.id}`, "the newer identified Run selected and shown again");
      await graphFollows(newer);
      await press(browser, ["Enter"], graphFocused, "the newer Run's graph focused again");
      await graphShows(newer);
    },
  ),

  // UX edges: E-MON-NOTICE
  // J-MON-CATALOG: the selected Run fills the graph area as its graph, beside the divider, also
  // while other Runs on the page cannot be read and the Run pane's notice says so.
  scenario<{ readable: string; unreadable: string[] }>(
    "navigation-unreadable-runs",
    "keeps the selected Run's graph within its pane while the notice names the Runs that cannot be read, in full on a click",
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
        const toggle = notice?.querySelector<HTMLElement>("button");
        const node = section?.querySelector<HTMLElement>('.svelte-flow__node[data-id="a:n000"]');
        const divider = document.querySelectorAll<HTMLElement>(".monitor [data-pane-resizer]")[1];
        if (!section || !heading || !notice || !toggle || !node || !divider) return null;
        const box = section.getBoundingClientRect();
        const within = (element: HTMLElement) => {
          const inner = element.getBoundingClientRect();
          return inner.width > 0 && inner.left >= box.left && inner.right <= box.right;
        };
        const line = divider.getBoundingClientRect();
        return {
          // The pane stays inside the window, and what it shows stays inside the pane.
          inWindow: box.left >= 0 && box.right <= window.innerWidth,
          bodyWithin: [...section.children].every((child) => within(child as HTMLElement)),
          nodeWithin: within(node),
          noticeWithin: within(notice),
          titleWhole: within(heading) && heading.scrollWidth <= heading.clientWidth,
          // The divider between the left side and the graph stays on screen, left of the pane.
          divider: line.width > 0 && line.left > 0 && line.right <= box.left + 1,
          text: toggle.textContent ?? "",
          cut: toggle.scrollWidth > toggle.clientWidth,
          open: toggle.getAttribute("aria-expanded") === "true",
          tall: notice.getBoundingClientRect().height > heading.getBoundingClientRect().height * 2,
        };
      }, `Run ${readable}`);
      let last: Awaited<ReturnType<typeof pane>> = null;
      await browser.waitUntil(async () => {
        last = await pane();
        return !!last && last.inWindow && last.bodyWithin && last.nodeWithin && last.noticeWithin && last.titleWhole && last.divider;
      }, { timeout: 15000, timeoutMsg: "the Run pane keeps its title, notice and graph within its width" })
        .catch((error: unknown) => { throw new Error(`${String(error)}; the pane last showed ${JSON.stringify(last)}`); });
      await graphShown(browser, "a:n000");
      const folded = (await pane())!;
      assert.ok(folded.cut && !folded.open, "the notice is one line, cut to the header's room");
      for (const id of unreadable) assert.match(folded.text, new RegExp(`Unreadable run ${id}: unknown variant \`dependency\``), `the notice names ${id}`);
      assert.doesNotMatch(folded.text, new RegExp(readable), "the readable Run is not named unreadable");
      // A click shows the notice in full; the graph stays within the pane. Another folds it.
      await browser.$(`section.panel[aria-label="Run ${readable}"] [role="status"] button`).click();
      await browser.waitUntil(async () => {
        const shown = await pane();
        return !!shown && shown.open && !shown.cut && shown.tall && shown.bodyWithin && shown.inWindow;
      }, { timeout: 15000, timeoutMsg: "the whole notice shown, wrapped within the pane" });
      await browser.$(`section.panel[aria-label="Run ${readable}"] [role="status"] button`).click();
      await browser.waitUntil(async () => {
        const shown = await pane();
        return !!shown && !shown.open && shown.cut;
      }, { timeout: 15000, timeoutMsg: "the notice folded back to one line" });
      await graphShown(browser, "a:n000");
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
      const { listbox, runIds, chosen, graphFocused } = paneNavigation(browser);
      await browser.waitUntil(async () => JSON.stringify(await runIds()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the repository's exact Runs" });
      await browser.waitUntil(() => listbox("Runs").isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      await press(browser, ["Enter"], graphFocused, "the graph focused");
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
      await press(browser, ["Enter"], graphFocused, "the narrowed Run's graph focused");
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
