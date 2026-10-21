// Launching the window on its one screen: the Pane of live branches beside the Main area, and
// what each way of starting the app opens first: nothing, a branch's last chip, the newest
// waiting review or a named one.
import assert from "node:assert/strict";
import { addRepository, addWorktrees, create, requests, signoff, type Fixture } from "../fixture.ts";
import { emptyRepository } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { detail, evalAt, graphShown, nodeAt, openArtifact } from "../graph-view.ts";
import { press } from "../keys.ts";
import { branch, branches, listed, mainTitle, scanned, selectBranch, waiting } from "../pane.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The native window's own title through Tauri's window API, not the page's document title. */
async function nativeTitle(browser: WebdriverIO.Browser): Promise<string> {
  type Tauri = { __TAURI__: { window: { getCurrentWindow(): { title(): Promise<string> } } } };
  // WebDriver resolves the returned promise inside the page.
  return await browser.execute(() => (window as unknown as Tauri).__TAURI__.window.getCurrentWindow().title()) as unknown as string;
}

/** The sign-off a repository added after `before` was read waits with. */
function added(fixture: Fixture, before: Set<string>): string {
  const found = requests(fixture).find((request) => request.evalId === "release/signoff" && !before.has(request.id));
  assert.ok(found, "the added repository's own sign-off waits");
  return found.id;
}

/** Three repositories, each with a waiting sign-off, oldest first. */
async function prepareReviews() {
  const fixture = await create();
  const ids = [signoff(fixture).id];
  for (const name of ["other", "third"]) {
    const before = new Set(requests(fixture).map((request) => request.id));
    await addRepository(fixture, name);
    ids.push(added(fixture, before));
  }
  return { fixture, ids };
}

export const scenarios: Scenario[] = [
  // UX journeys: J-LAUNCH J-PANE J-BRANCHES; UX edges: E-LAUNCH-MONITOR E-PANE-SELECT
  scenario<{ initialRun: string; worktreeRuns: string[] }>(
    "launch-monitor",
    "opens on the Pane of every live branch beside the Main area with nothing chosen, and a branch's name shows its last chip",
    async () => {
      const fixture = await create();
      const initialRun = signoff(fixture).runId;
      const worktreeRuns = await addWorktrees(fixture);
      await addRepository(fixture, "other");
      return { fixture, data: { initialRun, worktreeRuns } };
    },
    async (browser, { data: { initialRun, worktreeRuns } }) => {
      await waitFor(browser, async () => (await branches(browser)).length === 4, "the four live branches listed");
      assert.equal(await nativeTitle(browser), "Artifactize", "the native window is titled Artifactize");
      // One screen: the Pane's Search, Branches and Waiting stack beside the Main area, with no
      // Scope, Runs list or screen switch.
      const layout = await browser.execute(() => {
        const pane = document.querySelector<HTMLElement>('section[aria-label="Pane"]');
        const main = document.querySelector<HTMLElement>('section.panel[aria-label="Main area"]');
        const divider = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
        const box = (element: Element | null | undefined) => element?.getBoundingClientRect();
        const order = [...(pane?.querySelectorAll('input[aria-label="Search branches and reviews"], [aria-label="Branches"], section[aria-label="Waiting"]') ?? [])]
          .map((part) => part.getAttribute("aria-label"));
        return {
          order,
          query: pane?.querySelector<HTMLInputElement>("input")?.value,
          beside: !!pane && !!main && box(pane)!.right <= box(main)!.left + 1,
          divider: !!divider && box(divider)!.width > 0 && box(divider)!.left >= box(pane)!.right - 1,
          scope: !!document.querySelector('[aria-label="Scope"], [aria-label="Runs"], [aria-label="Screens"]'),
          empty: main?.querySelector(".empty")?.textContent?.trim(),
        };
      });
      assert.deepEqual(layout, {
        order: ["Search branches and reviews", "Branches", "Waiting"],
        query: "sort:recent",
        beside: true,
        divider: true,
        scope: false,
        empty: "Choose a branch or a waiting review in the Pane.",
      });
      assert.equal(await mainTitle(browser), "Nothing chosen");
      // Every live branch of every project with Runs, read as project and branch.
      assert.deepEqual((await branches(browser)).map((entry) => entry.name).sort(),
        ["other  master", "repo  master", "repo  scope-one", "repo  scope-two"]);
      assert.equal((await branches(browser)).filter((entry) => entry.selected).length, 0, "nothing selected yet");
      // Each branch's row holds its own Run in progress as its last chip.
      for (const [name, run] of [["repo  master", initialRun], ["repo  scope-one", worktreeRuns[0]], ["repo  scope-two", worktreeRuns[1]]]) {
        assert.deepEqual((await branch(browser, name))?.chips.map((chip) => chip.id), [run], `${name}'s one Run`);
      }
      // A click on a branch's name selects it and shows its last chip, filled.
      await selectBranch(browser, "repo  master");
      await waitFor(browser, async () => (await mainTitle(browser)) === `Run ${initialRun}`, "the branch's running Run shown");
      assert.equal((await branch(browser, "repo  master"))?.chips[0].filled, true, "the shown chip is filled");
      await graphShown(browser, "a:release");
      // The shown Run's own verdicts: release waits for its Human, ship waits, docs is GREEN.
      for (const [id, label] of [["a:release", "release: in progress"], ["a:ship", "ship: waiting"], ["a:docs", "docs: GREEN"]]) {
        await waitFor(browser, async () => (await browser.$(nodeAt(id)).getAttribute("aria-label")) === label, `${id} drawn as ${label}`);
      }
      for (const [artifact, path, glyph, status] of [
        ["release", "e:release/signoff", "?", /Human sign-off · unclaimed/],
        ["ship", "e:ship/check", "…", /waits for release/],
        ["docs", "e:docs/lint", "✓", /^GREEN(?: · reused)?$/],
      ] as const) {
        await openArtifact(browser, `a:${artifact}`, `Artifact ${artifact}`);
        const shown = await browser.execute((selector) => {
          const row = document.querySelector(selector);
          return { glyph: row?.querySelector(".glyph")?.textContent?.trim(), status: row?.querySelector(".status")?.textContent?.trim() };
        }, evalAt(path));
        assert.equal(shown.glyph, glyph, `${path}'s verdict glyph`);
        assert.match(shown.status ?? "missing row", status, `${path}'s own status`);
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, `${artifact}'s Detail closed`);
      }
      // Another branch's name shows that branch's own Run instead.
      await selectBranch(browser, "repo  scope-one");
      await waitFor(browser, async () => (await mainTitle(browser)) === `Run ${worktreeRuns[0]}`, "scope-one's own Run shown");
      assert.equal((await branch(browser, "repo  master"))?.selected, false, "one branch selected at a time");
    },
  ),

  // UX journey: J-LAUNCH; UX edge: E-LAUNCH-REPO
  scenario<{ run: string }>(
    "launch-repo",
    "starts with --repo on that branch selected, its last chip (the Run in progress) shown",
    async () => {
      const fixture = await create();
      const run = signoff(fixture).runId;
      await addRepository(fixture, "other");
      return { fixture, args: ["--repo", fixture.repo], data: { run } };
    },
    async (browser, { data: { run } }) => {
      await waitFor(browser, async () => (await mainTitle(browser)) === `Run ${run}`, "the branch's last chip shown");
      const shown = await branch(browser, "repo  master");
      assert.equal(shown?.selected, true, "the branch of --repo is selected");
      assert.deepEqual(shown?.chips.map((chip) => [chip.id, chip.filled]), [[run, true]], "its last chip, the running Run, is filled");
      assert.equal((await branch(browser, "other  master"))?.selected, false, "no other branch is selected");
      await graphShown(browser, "a:release");
    },
  ),

  // UX journey: J-LAUNCH; UX edge: E-LAUNCH-REPO
  scenario<Record<string, never>>(
    "launch-repo-without-runs",
    "lists the branch of --repo before its project has a Run, its Current chip shown",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", emptyRepository(fixture)], data: {} };
    },
    async (browser) => {
      await listed(browser, "empty-repo  master");
      const shown = await branch(browser, "empty-repo  master");
      assert.equal(shown?.selected, true);
      assert.deepEqual(shown?.chips.map((chip) => [chip.id, chip.filled]), [["current", true]], "a branch without Runs ends with its Current chip");
      await waitFor(browser, async () => (await mainTitle(browser)) === "Current · empty-repo  master", "its Current shown");
      await waitFor(browser, async () => /^Scanned at /.test(await scanned(browser)), "its worktree scanned");
      // The project with Runs is listed too; the new one, without Runs, sorts last.
      assert.deepEqual((await branches(browser)).map((entry) => entry.name), ["repo  master", "empty-repo  master"]);
    },
  ),

  // UX journeys: J-LAUNCH J-WAITING; UX edge: E-LAUNCH-REVIEW
  scenario<{ ids: string[] }>(
    "launch-review",
    "starts with review on the newest waiting review of every repository, open in the Main area",
    async () => {
      const { fixture, ids } = await prepareReviews();
      return { fixture, args: ["review"], data: { ids } };
    },
    async (browser, { data: { ids } }) => {
      const newest = ids[2];
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "a Review in the Main area");
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${newest} · WAITING_HUMAN · unclaimed`),
        "the newest waiting review opened");
      assert.equal(await nativeTitle(browser), "Artifactize");
      const stack = await waiting(browser);
      assert.equal(stack.bar, "Waiting (3)");
      assert.deepEqual(stack.rows.map((row) => row.id), [...ids].reverse(), "every repository's waiting review, newest at the top");
      assert.deepEqual(stack.rows.map((row) => row.where), ["third  master", "other  master", "repo  master"]);
      assert.equal(stack.rows[0].selected, true, "the opened review is the selected entry");
    },
  ),

  // UX journey: J-LAUNCH; UX edge: E-LAUNCH-REVIEW-ID
  scenario<{ ids: string[] }>(
    "launch-review-id",
    "starts with review ID on that review, not the newest one",
    async () => {
      const { fixture, ids } = await prepareReviews();
      return { fixture, args: ["review", ids[1]], data: { ids } };
    },
    async (browser, { data: { ids } }) => {
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "a Review in the Main area");
      await waitFor(browser, async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${ids[1]} · WAITING_HUMAN · unclaimed`),
        "the named review opened");
      assert.deepEqual((await waiting(browser)).rows.map((row) => row.id), [...ids].reverse(), "the newest one stays at the top, not opened");
    },
  ),
];
