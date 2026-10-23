// Moving through the window in the real app: the keys of what has focus, a refresh, the
// pointer that only highlights, the Pane's keys through branches, timelines and waiting reviews
// into the Main area and back, a Detail stepped back out of, unreadable Runs beside a readable
// one; and quitting a window that holds no claim.
import assert from "node:assert/strict";
import { addWaiting, create, createHistory, createNavigationFixture, createUnreadableRuns, orderRuns, requests, verifyWaiting } from "../fixture.ts";
import { git } from "../environment.ts";
import { press } from "../keys.ts";
import { checkpoint, refreshed } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { sessionEndedByApp } from "../observation.ts";
import { nativePath, primaryModifier, readText, requestWindowClose, shortcutLabel, writeText } from "../platform.ts";
import { visibleCenter } from "../pointer.ts";
import { aside, detail, fact, graphShown, nodeAt, openArtifact, openTab, summary } from "../graph-view.ts";
import { branch, branches, chooseChip, entriesFocused, listed, MAIN, mainFocused, mainTitle, scanned, showRun, waiting } from "../pane.ts";

type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture"> & { review: string };

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The keys the open Help dialog lists, each with what it does. */
function shortcuts(browser: WebdriverIO.Browser) {
  return browser.execute(() => [...document.querySelectorAll('[role="dialog"].help dt')].map((key) => ({
    keys: key.textContent?.trim() ?? "",
    action: key.nextElementSibling?.textContent?.trim() ?? "",
  })));
}

export const scenarios: Scenario[] = [
  // UX journey: J-HELP; UX edges: E-HELP-OPEN E-HELP-CLOSE
  scenario<Record<string, never>>(
    "navigation-help",
    "lists the keys of the Pane, the Main area or Settings, whichever has focus, with ? and closes them with Esc",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      // Help, not Settings, which is also a dialog.
      const helpOpen = () => browser.$('[role="dialog"].help').isExisting();
      /** Each pair's keys as a pattern, or as the exact label the platform shows. */
      const listed = async (pairs: [RegExp | string, RegExp][], absent: RegExp[], where: string) => {
        await press(browser, ["?"], helpOpen, `the keys of ${where}`);
        assert.match(await browser.$('[role="dialog"].help').getText(), /Keyboard shortcuts/);
        const shown = await shortcuts(browser);
        for (const [keys, action] of pairs) {
          const matches = (label: string) => typeof keys === "string" ? label === keys : keys.test(label);
          assert.ok(shown.some((entry) => matches(entry.keys) && action.test(entry.action)), `${where}'s Help pairs ${keys} with ${action}: ${JSON.stringify(shown)}`);
        }
        for (const keys of absent) assert.ok(!shown.some((entry) => keys.test(entry.keys)), `${where}'s Help leaves out ${keys}`);
        await press(browser, ["Escape"], async () => !(await helpOpen()), `${where}'s keys closed`);
      };
      await graphShown(browser, "a:release");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await listed([
        [/^↑ \/ ↓ · j \/ k$/, /Select a branch or a timeline entry, then a waiting review/],
        [/^← \/ →$/, /Fold \/ expand the selected branch/],
        [/^Esc$/, /sort:recent/],
        [/\bF5\b/, /^Refresh/],
        [shortcutLabel("Ctrl+O"), /^Open a request by ID$/],
        [shortcutLabel("/ · Ctrl+F"), /^Search branches and reviews$/],
      ], [/^Click an Artifact$/, /^c$/], "the Pane");
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await listed([
        [/^Click an Artifact$/, /Open its Detail/],
        [/^Click the background$/, /Close the Detail/],
        [/^Click a tab or an eval$/, /Show the Artifact or that eval/],
        [/^c$/, /^Claim$/],
        [/\bF5\b/, /^Refresh/],
      ], [/^↑ \/ ↓ · j \/ k$/, /^v$/, /^(?:!|o|b \/ Backspace)$/], "the Main area");
      const beneath = await mainTitle(browser);
      await press(browser, [primaryModifier, ","], () => browser.$("#settings-title").isExisting(), "Settings open");
      assert.equal(await mainTitle(browser), beneath, "Settings opens over the Main area's view");
      await listed([[/^Esc$/, /^Close Settings$/], [/^← \/ →$/, /Providers and Remote tabs/]], [/^Click an Artifact$/, /^c$/], "Settings");
    },
  ),

  // UX journey: J-LIVE; UX edge: E-MON-LIVE
  scenario<{ run: string }>(
    "navigation-live-run",
    "shows a Run another process starts on the shown branch without a reload",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: { run: requests(fixture)[0].runId } };
    },
    async (browser, { fixture, data: { run } }) => {
      await graphShown(browser, "a:release");
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id), [run]);
      const id = await verifyWaiting(fixture);
      // Explicitly the newer, whatever times the two verifies were saved at.
      orderRuns(fixture, [run, id]);
      await waitFor(browser, async () => JSON.stringify((await branch(browser, "repo  master"))?.chips.map((chip) => chip.id)) === JSON.stringify([id, run]),
        "the new Run arrives in the timeline without F5");
      assert.equal(await mainTitle(browser), `Run ${run}`, "the shown Run stays shown");
    },
  ),

  // UX journeys: J-REFRESH J-MAIN-CURRENT; UX edges: E-REFRESH E-CURRENT-REFRESH
  scenario<{ worktree: string }>(
    "navigation-refresh",
    "reloads the state, scans the branches and the shown Current again with F5 and Ctrl+R, on the same view",
    async () => {
      const fixture = await create();
      git(fixture.repo, "-C", fixture.repo, "add", ".");
      git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Refresh fixture inputs");
      return { fixture, args: ["--repo", fixture.repo], data: { worktree: nativePath(fixture.root, "after-refresh") } };
    },
    async (browser, { fixture, data }) => {
      await graphShown(browser, "a:release");
      const run = requests(fixture)[0].runId;
      // The Run shown: a refresh keeps it shown, and finds a worktree Git alone added.
      git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "after-refresh", data.worktree);
      let before = await checkpoint(browser);
      await press(browser, ["F5"], async () => (await checkpoint(browser)).activity.refresh?.generation === (before.activity.refresh?.generation ?? 0) + 1,
        "F5's refresh shown", { handled: true });
      await refreshed(browser, before);
      assert.ok(await branch(browser, "repo  after-refresh"), "the refresh scanned the branches again: the new worktree listed");
      assert.equal(await mainTitle(browser), `Run ${run}`, "on the same view");
      assert.equal(await browser.$(`${MAIN} > header [role="status"]`).isExisting(), false, "the refresh read without an error");
      // The new branch's Current: only a refresh or its Refresh button scans it again.
      await chooseChip(browser, "repo  after-refresh", "current");
      await waitFor(browser, async () => /^Scanned at /.test(await scanned(browser)), "its Current scanned");
      const structure = () => browser.$('section[aria-label="Current"] pre').getText();
      await browser.$('section[aria-label="Current"] details summary').click();
      const declaration = nativePath(data.worktree, "docs", "index.artf");
      for (const [index, keys] of [["F5"], [primaryModifier, "r"]].entries()) {
        const title = `Refreshed docs ${index}`;
        writeText(declaration, readText(declaration).replace(/title = "[^"]*"/, `title = "${title}"`));
        assert.doesNotMatch(await structure(), new RegExp(title), "the changed declaration is not shown before the refresh");
        before = await checkpoint(browser);
        await press(browser, keys, async () => (await structure()).includes(title), `${keys.join("+")} scanned the Current again`, { handled: true });
        await refreshed(browser, before);
        assert.equal(await mainTitle(browser), "Current · repo  after-refresh", "on the same view");
      }
    },
  ),

  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-TECHNICAL E-MON-DETAIL-BACK
  scenario<Record<string, never>>(
    "navigation-technical-detail",
    "shows the Debug facts of an eval's Summary with t and closes the Detail back to the graph",
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
      await browser.waitUntil(async () => (await summary(browser)).instruction.startsWith("Ship"), { timeout: 15000, timeoutMsg: "ship/check's Summary" });
      assert.equal((await summary(browser)).debug, false, "Debug starts folded");
      assert.equal(await fact(browser, "Request"), "");
      assert.equal(await fact(browser, "Waits for"), "", "the waiting eval's input facts start hidden");
      await aside(browser).$('[role="document"]').click();
      await press(browser, ["t"], async () => (await summary(browser)).debug === true && (await fact(browser, "Request")) === request.id
        && (await fact(browser, "Waits for")).includes("release"), "the request and its waiting input inside Debug");
      assert.match(await fact(browser, "Waits for"), /signoff/);
      await press(browser, ["t"], async () => (await summary(browser)).debug === false && (await fact(browser, "Request")) === ""
        && (await fact(browser, "Waits for")) === "", "the Debug facts and waits hidden again");
      assert.ok((await summary(browser)).instruction.startsWith("Ship"), "folding Debug keeps the Summary");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // The graph as it was, every Artifact still drawn and reachable.
      for (const id of ["a:release", "a:ship", "a:docs"]) await graphShown(browser, id);
    },
  ),

  // The pointer rule of the UX journeys: hovering opens nothing; information opens on a click.
  scenario<Record<string, never>>(
    "navigation-hover-opens-nothing",
    "opens nothing for the Artifact or the timeline entry under the pointer, and opens the Artifact's Detail on a click",
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
          const target = (event.target as Element | null)?.closest?.(".svelte-flow__node, button[data-chip]");
          if (target) log.push(target.getAttribute("data-id") ?? target.getAttribute("data-chip") ?? "");
        }, true);
      });
      const look = () => browser.execute((selector) => ({
        over: (window as unknown as { overs: string[] }).overs.slice(),
        opened: !!document.querySelector('aside[aria-label="Detail"], [aria-label="Summary"], [aria-label^="Evals of "], [role="tooltip"], [role="menu"]'),
        tooltip: !!document.querySelector(selector)?.querySelector("[title]") || !!document.querySelector(selector)?.closest("[title]")
          || !!document.querySelector("button[data-chip][title]"),
      }), nodeAt("a:ship"));
      const chip = await browser.$("button[data-chip]");
      await browser.action("pointer").move(await visibleCenter(browser, await chip.getElement())).perform();
      await browser.waitUntil(async () => (await look()).over.length > 0, { timeout: 15000, timeoutMsg: "the pointer moved over a timeline entry" });
      const point = await visibleCenter(browser, await browser.$(nodeAt("a:ship")).getElement());
      await browser.action("pointer").move(point).perform();
      await browser.waitUntil(async () => (await look()).over.includes("a:ship"), { timeout: 15000, timeoutMsg: "the pointer moved over ship" });
      const hovered = await look();
      assert.equal(hovered.opened, false, "hovering opens no Detail, summary, eval list, tooltip or menu");
      assert.equal(hovered.tooltip, false, "neither the Artifact nor a timeline entry carries a tooltip");
      // The same probe sees what a click opens.
      await browser.$(nodeAt("a:ship")).click();
      await browser.waitUntil(async () => (await look()).opened && (await detail(browser))?.title === "Artifact ship",
        { timeout: 15000, timeoutMsg: `a click opened ship's Detail: ${JSON.stringify(await detail(browser))}` });
    },
  ),

  // UX journeys: J-PANE-NAVIGATE J-WAITING; UX edges: E-MON-SCOPE-MOVE E-MON-EXPAND E-MON-RUNS-MOVE
  // E-MON-RUNS-GRAPH E-MON-GRAPH-TO-RUNS E-REVIEW-OPEN E-HUMAN-ESC-DETAIL
  scenario<NavigationData>(
    "navigation-pane-keys",
    "moves through the branches and then the waiting reviews by keys, expands a branch and moves through its timeline, folds it, enters the Main area and comes back",
    async () => {
      const { fixture, ...data } = await createNavigationFixture();
      const [review] = await addWaiting(fixture, "gates", 1);
      return { fixture, data: { ...data, review } };
    },
    async (browser, { data: example }) => {
      const [older, newer] = example.runs;
      // The most recent Run first: the gates (now), other (third), then repo.
      await waitFor(browser, async () => (await branches(browser)).length === 3, "the three live branches");
      assert.deepEqual((await branches(browser)).map((entry) => entry.name), ["gates  master", "other  master", "repo  master"]);
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      const selected = async () => (await branches(browser)).find((entry) => entry.selected)?.name
        ?? (await waiting(browser)).rows.find((entry) => entry.selected)?.id ?? null;
      const filled = async (name: string) => (await branch(browser, name))?.chips.find((chip) => chip.filled)?.id ?? null;
      const expanded = async (name: string) => (await branch(browser, name))?.expanded === true;
      // ↓ and j select the next branch, which shows its newest entry; k and ↑ go back.
      await press(browser, ["ArrowDown"], async () => (await selected()) === "gates  master" && /^Run /.test(await mainTitle(browser)), "the first branch's Run in progress shown");
      await press(browser, ["j"], async () => (await selected()) === "other  master" && (await mainTitle(browser)) === "Current · other  master", "the next branch's Current shown");
      await press(browser, ["ArrowDown"], async () => (await selected()) === "repo  master" && (await mainTitle(browser)) === "Current · repo  master", "repo's Current shown");
      // → expands it; → again enters its timeline on its Current; ↓ and ↑ move through its Runs,
      // each shown and marked.
      await press(browser, ["ArrowRight"], () => expanded("repo  master"), "repo expanded");
      await press(browser, ["ArrowRight"], async () => (await filled("repo  master")) === "current", "on its Current");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === `Run ${newer.id}` && (await filled("repo  master")) === newer.id, "the newer Run shown");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === `Run ${older.id}` && (await filled("repo  master")) === older.id, "the older Run shown");
      await press(browser, ["ArrowUp"], async () => (await filled("repo  master")) === newer.id, "the newer Run again");
      await press(browser, ["ArrowUp"], async () => (await filled("repo  master")) === "current", "its Current again");
      await press(browser, ["ArrowDown"], async () => (await filled("repo  master")) === newer.id, "back on the newer Run");
      // ← steps back out to the branch, keeping what is shown; ← again folds it.
      await press(browser, ["ArrowLeft"], async () => (await browser.execute(() =>
        document.querySelector('[aria-label="Branches"] > [aria-selected="true"]')?.getAttribute("aria-expanded"))) === "true", "the branch itself selected");
      await press(browser, ["ArrowLeft"], async () => !(await expanded("repo  master")), "repo folded");
      assert.equal(await mainTitle(browser), `Run ${newer.id}`, "stepping out and folding keep what is shown");
      // Enter goes into the Main area, on the shown Run: its own historical criterion.
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await graphShown(browser, "a:subject");
      const opened = await openArtifact(browser, "a:subject", "Artifact subject");
      assert.deepEqual(opened.evals.map((row) => row.name), [newer.eval], "only the shown Run's criterion");
      await openTab(browser, `e:subject/${newer.eval}`);
      const human = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"][tabindex="0"]');
      await waitFor(browser, () => browser.execute((name) => document.querySelector('[aria-label="Human review"] h3')?.textContent?.includes(`subject/${name}`) ?? false,
        newer.eval), "the eval's tab holds its Human review");
      await human().click();
      // Esc leaves the review's Detail, back on the graph; Esc again returns to the Pane.
      await press(browser, ["Escape"], async () => (await detail(browser)) === null && await mainFocused(browser), "the Detail closed, the Main area focused");
      await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane");
      assert.equal(await selected(), "repo  master", "the same entry stays selected");
      // Past the last branch the waiting reviews: one is only selected, Enter opens it.
      await press(browser, ["j"], async () => (await selected()) === example.review, "the waiting review selected");
      assert.equal(await mainTitle(browser), `Run ${newer.id}`, "selecting a waiting review opens nothing");
      await press(browser, ["Enter"], async () => (await mainTitle(browser)) === "Review", "Enter opened the review");
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${example.review} · WAITING_HUMAN`),
        "the selected review opened");
      // Esc in a review opened from the Pane returns to the Pane, the review still selected.
      await browser.$('[aria-label="Human review"]').click();
      await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane from the review");
      assert.equal(await selected(), example.review);
      await press(browser, ["k"], async () => (await selected()) === "repo  master", "k back to the last branch");
    },
  ),

  // UX journey: J-PANE-NAVIGATE; UX edges: E-MON-RUNS-MOVE E-MON-RUNS-OLDER
  scenario<{ runs: string[] }>(
    "navigation-timeline-keys",
    "moves by ↓ and ↑ through a timeline longer than the Pane, the list scrolling so that each chosen entry shows, and asks for older Runs past the oldest",
    async () => {
      const { fixture, runs } = createHistory(30);
      return { fixture, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      const LIST = '[role="tree"][aria-label="Branches"]';
      /** The list's scroll offset, whether it overflows, and the marked entry with whether all of it shows within the list. */
      const shown = () => browser.execute((selector) => {
        const list = document.querySelector<HTMLElement>(selector)!;
        const box = list.getBoundingClientRect();
        const entry = list.querySelector<HTMLElement>('button[data-chip][aria-pressed="true"]');
        const at = entry?.getBoundingClientRect();
        return {
          scroll: Math.round(list.scrollTop),
          overflows: list.scrollHeight > list.clientHeight,
          filled: entry?.dataset.chip ?? null,
          visible: !!at && at.top >= box.top - 1 && at.bottom <= box.bottom + 1,
        };
      }, LIST);
      await listed(browser, "repo  master");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      await press(browser, ["ArrowDown"], async () => (await mainTitle(browser)) === "Current · repo  master", "the branch's Current shown");
      if (!(await branch(browser, "repo  master"))?.expanded) await press(browser, ["ArrowRight"], async () => (await branch(browser, "repo  master"))?.expanded === true, "expanded");
      await press(browser, ["ArrowRight"], async () => (await shown()).filled === "current", "on its Current");
      const held = (await branch(browser, "repo  master"))!.chips.length - 1;
      assert.ok(held < runs.length, `older Runs are left for later: ${held} of ${runs.length}`);
      const start = (await shown()).scroll;
      // On through every Run, newest to oldest, downwards: each chosen entry shows within the list.
      // On the oldest held, ↓ first lists older Runs below it, the selection staying; ↓ again
      // moves on into them.
      for (const id of [...runs].reverse()) {
        const held = (await branch(browser, "repo  master"))!.chips;
        const at = (await shown()).filled;
        if (held[held.length - 1].id === at) {
          const title = await mainTitle(browser);
          await press(browser, ["ArrowDown"], async () => (await branch(browser, "repo  master"))!.chips.length > held.length, "older Runs listed below the oldest held");
          assert.equal((await shown()).filled, at, "the selection stays while older Runs are listed");
          assert.equal(await mainTitle(browser), title, "the Main area still shows the selected entry");
        }
        await press(browser, ["ArrowDown"], async () => (await shown()).filled === id && (await mainTitle(browser)) === `Run ${id}`, `${id} chosen`);
        await waitFor(browser, async () => (await shown()).visible, `${id} shows within the list: ${JSON.stringify(await shown())}`);
      }
      assert.equal((await branch(browser, "repo  master"))!.chips.length, runs.length + 1, "the older Runs arrived on the way");
      assert.ok((await shown()).overflows && (await shown()).scroll > start, "the list scrolled toward the oldest to show them");
      // Back up to the Current, the list scrolling back to show each.
      for (const id of [...runs.slice(1), "current"]) {
        await press(browser, ["ArrowUp"], async () => (await shown()).filled === id, `${id} chosen again`);
        await waitFor(browser, async () => (await shown()).visible, `${id} shows within the list: ${JSON.stringify(await shown())}`);
      }
      assert.equal(await mainTitle(browser), "Current · repo  master");
    },
  ),

  // UX journey: J-MAIN-RUN; UX edge: E-MON-NOTICE
  scenario<{ readable: string; unreadable: string[] }>(
    "navigation-unreadable-runs",
    "keeps a shown Run's graph within the Main area while its notice names the Runs that cannot be read, in full on a click",
    async () => {
      const { fixture, readable, unreadable, restore } = createUnreadableRuns();
      return { fixture, args: ["--repo", fixture.repo], data: { readable, unreadable }, release: async () => restore() };
    },
    async (browser, { data: { readable, unreadable } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", readable);
      const pane = () => browser.execute((main) => {
        const section = document.querySelector<HTMLElement>(main);
        const heading = section?.querySelector<HTMLElement>("h2");
        const notice = section?.querySelector<HTMLElement>(':scope > header [role="status"]');
        const toggle = notice?.querySelector<HTMLElement>("button");
        const node = section?.querySelector<HTMLElement>('.svelte-flow__node[data-id="a:n000"]');
        const divider = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
        if (!section || !heading || !notice || !toggle || !node || !divider) return null;
        const box = section.getBoundingClientRect();
        const within = (element: HTMLElement) => {
          const inner = element.getBoundingClientRect();
          return inner.width > 0 && inner.left >= box.left && inner.right <= box.right;
        };
        const line = divider.getBoundingClientRect();
        return {
          inWindow: box.left >= 0 && box.right <= window.innerWidth,
          bodyWithin: [...section.children].every((child) => within(child as HTMLElement)),
          graph: (() => { const at = section.querySelector(".graph")!.getBoundingClientRect(); return [at.left, at.top, at.width, at.height].map(Math.round); })(),
          nodeWithin: within(node),
          noticeWithin: within(notice),
          titleWhole: within(heading) && heading.scrollWidth <= heading.clientWidth,
          // The divider between the Pane and the Main area stays on screen, left of it.
          divider: line.width > 0 && line.left > 0 && line.right <= box.left + 1,
          text: toggle.textContent ?? "",
          cut: toggle.scrollWidth > toggle.clientWidth,
          open: toggle.getAttribute("aria-expanded") === "true",
          tall: toggle.getBoundingClientRect().height > heading.getBoundingClientRect().height * 2,
        };
      }, MAIN);
      let last: Awaited<ReturnType<typeof pane>> = null;
      await browser.waitUntil(async () => {
        last = await pane();
        return !!last && last.inWindow && last.bodyWithin && last.nodeWithin && last.noticeWithin && last.titleWhole && last.divider;
      }, { timeout: 15000, timeoutMsg: "the Main area keeps its title, notice and graph within its width" })
        .catch((error: unknown) => { throw new Error(`${String(error)}; the Main area last showed ${JSON.stringify(last)}`); });
      await graphShown(browser, "a:n000");
      const folded = (await pane())!;
      assert.ok(folded.cut && !folded.open, "the notice is one line, cut to the header's room");
      for (const id of unreadable) assert.match(folded.text, new RegExp(`Unreadable run ${id}: unknown variant \`dependency\``), `the notice names ${id}`);
      assert.doesNotMatch(folded.text, new RegExp(readable), "the readable Run is not named unreadable");
      // A click shows the notice in full; the graph stays where it was. Another folds it.
      const toggle = () => browser.$(`${MAIN} > header [role="status"] button`);
      await toggle().click();
      await browser.waitUntil(async () => {
        const shown = await pane();
        return !!shown && shown.open && !shown.cut && shown.tall && shown.bodyWithin && shown.inWindow;
      }, { timeout: 15000, timeoutMsg: "the whole notice shown, wrapped within the header" });
      assert.deepEqual((await pane())?.graph, folded.graph, "opening the full notice does not move or resize the graph");
      await graphShown(browser, "a:n000");
      await toggle().click();
      await browser.waitUntil(async () => {
        const shown = await pane();
        return !!shown && !shown.open && shown.cut;
      }, { timeout: 15000, timeoutMsg: "the notice folded back to one line" });
      assert.deepEqual((await pane())?.graph, folded.graph, "collapsing the notice keeps the same graph geometry");
      await graphShown(browser, "a:n000");
    },
  ),

  // UX journey: J-QUIT; UX edge: E-QUIT-CLEAN
  scenario<Record<string, never>>(
    "navigation-quit-clean",
    "closes the window at once with q",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await waitFor(browser, () => entriesFocused(browser), "the Pane's entries focused");
      // Observe the real window's exit before retiring the ended WebDriver client session.
      await sessionEndedByApp(browser, () => browser.keys(["q"]));
    },
  ),

  // UX journey: J-QUIT; UX edge: E-QUIT-CLEAN
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
