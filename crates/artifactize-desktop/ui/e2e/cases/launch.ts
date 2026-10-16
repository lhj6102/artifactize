// Launching and the window: the Monitor's catalog of
// repositories, scopes, Runs and verdicts, the screen shortcuts, and the Review screen that
// starts on the waiting sign-offs of every repository.
import assert from "node:assert/strict";
import { addRepository, addSearchExamples, addWorktrees, artifactize, create, requests } from "../fixture.ts";
import { fill, press } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";

import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

async function prepareMonitor() {
  const fixture = await create();
  const initialRun = requests(fixture)[0].runId;
  const worktreeRuns = await addWorktrees(fixture);
  await addRepository(fixture, "other");
  await addRepository(fixture, "third");
  return { fixture, args: ["--repo", fixture.repo], data: { initialRun, worktreeRuns } };
}

async function prepareRepositories(reviewScreen = false) {
  const fixture = await create();
  await addRepository(fixture, "other");
  await addRepository(fixture, "third");
  return { fixture, args: reviewScreen ? ["review"] : [], data: {} };
}

type PreviewExpectation = { title: string; instruction: string };

async function preparePreviews() {
  const definitions = [
    { name: "repo", title: "Primary release approval", instruction: "Review the primary release notes for this preview." },
    { name: "other", title: "Other release approval", instruction: "Review the other repository's release notes for this preview." },
    { name: "third", title: "Third release approval", instruction: "Review the third repository's release notes for this preview." },
  ];
  const fixture = await create(definitions[0]);
  const previews: Record<string, PreviewExpectation> = {};
  for (const [index, definition] of definitions.entries()) {
    if (index > 0) await addRepository(fixture, definition.name, definition);
    const added = requests(fixture).filter((request) => request.evalId === "release/signoff" && !(request.id in previews));
    assert.equal(added.length, 1, `one new ${definition.name} sign-off was prepared`);
    previews[added[0].id] = { title: `release/signoff · ${definition.title}`, instruction: definition.instruction };
  }
  return { fixture, args: ["review"], data: { previews } };
}

function monitor(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);
  const scopes = () =>
    browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')].map((row) => (row as HTMLElement).innerText.replace(/\s+/g, " ").trim()));
  const active = () => browser.execute(() => document.querySelector('[role="tab"][data-state="active"]')?.textContent ?? "");
  const settingsShown = () => browser.execute(() => !!document.querySelector(".settings h1") && (document.querySelector(".settings") as HTMLElement).offsetParent !== null);
  return { text, scopes, active, settingsShown };
}

function allMonitor(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);
  const selectedScope = () =>
    browser.execute(() => (document.querySelector('[aria-label="Scope"] [aria-selected="true"]') as HTMLElement | null)?.innerText ?? "");
  return { text, selectedScope };
}

function review(browser: WebdriverIO.Browser) {
  const rows = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')].map((row) => (row as HTMLElement).innerText.replace(/\s+/g, " ").trim()),
    );
  const query = () => browser.execute(() => (document.querySelector('[aria-label="Search requests"]') as HTMLInputElement).value);
  const preview = () => browser.execute(() => [...document.querySelectorAll("section.panel h3")].at(-1)?.textContent?.trim() ?? "");
  return { rows, query, preview };
}

/** The native window's own title through Tauri's window API, not the page's document title. */
async function nativeTitle(browser: WebdriverIO.Browser): Promise<string> {
  type Tauri = { __TAURI__: { window: { getCurrentWindow(): { title(): Promise<string> } } } };
  // WebDriver resolves the returned promise inside the page.
  return await browser.execute(() => (window as unknown as Tauri).__TAURI__.window.getCurrentWindow().title()) as unknown as string;
}

export const scenarios: Scenario[] = [
  // UX edges: E-MON-SCOPE-MOVE E-MON-RUNS-MOVE
  scenario<{ initialRun: string; worktreeRuns: string[] }>(
    "launch-catalog",
    "catalogs the repositories, their Runs and the verdicts of a Run's tree",
    async () => prepareMonitor(),
    async (browser, { data }) => {
      const { text, scopes } = monitor(browser);
      const { initialRun } = data;
      await waitFor(browser, async () => (await text()).includes("signoff"), "the Run tree");
      assert.equal(await nativeTitle(browser), "Artifactize", "the native window opened on the Monitor is titled Artifactize");
      const entries = await scopes();
      assert.ok(entries.some((entry) => entry.startsWith("ALL")), `ALL is listed: ${entries.join(" | ")}`);
      assert.ok(entries.some((entry) => entry.includes("other")), "the other repository is listed");
      assert.ok(entries.some((entry) => entry.includes("third")), "the third repository is listed");
      // Identify the saved Run before checking the verdicts of its own tree rows.
      const selectedRun = await browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0]);
      assert.equal(selectedRun, initialRun);
      const docs = await browser.$('[role="treeitem"][data-path="a:docs"]');
      await browser.$('[role="tree"]').click();
      await docs.click();
      await press(browser, ["l"], async () => (await docs.getAttribute("aria-expanded")) === "true", "the completed docs eval unfolded");
      const verdict = (path: string) => browser.execute((id) => {
        const row = [...document.querySelectorAll('[role="treeitem"]')].find((entry) => entry.getAttribute("data-path") === id);
        return {
          glyph: row?.querySelector(".glyph")?.textContent?.trim(),
          status: row?.querySelector(".status")?.textContent?.trim(),
        };
      }, path);
      // This fixture has a waiting Human release, its waiting consumer and GREEN docs.
      // The Artifact summaries and their individual evals must each display that state.
      for (const [path, glyph, status] of [
        ["a:release", "?", /in progress: signoff/],
        ["a:release/e:release/signoff", "?", /Human sign-off · unclaimed/],
        ["a:ship", "…", /waits for release/],
        ["a:ship/e:ship/check", "…", /waits for release/],
        ["a:docs", "✓", /^$/],
        ["a:docs/e:docs/lint", "✓", /^GREEN(?: · reused)?$/],
      ] as const) {
        const shown = await verdict(path);
        assert.equal(shown.glyph, glyph, `${path}'s verdict glyph in ${initialRun}`);
        assert.match(shown.status ?? "missing row", status, `${path}'s own status in ${initialRun}`);
      }
      // Another scope lists its own Runs.
      const runIds = () =>
        browser.execute(() =>
          [...document.querySelectorAll('[aria-label="Runs"] [role="option"]')].map((row) => /run-\S+/.exec((row as HTMLElement).innerText)?.[0] ?? ""),
        );
      const before = await runIds();
      const other = await browser.$('//ul[@aria-label="Scope"]//li[contains(., "other")]');
      await other.click();
      await waitFor(browser, async () => (await browser.$('nav[aria-label="Breadcrumb"]').getText()).includes("other"), "the other scope selected");
      await waitFor(browser, async () => /Runs \(1\)/i.test(await text()), "its one Run");
      // The Runs pane now lists that repository's own Run, not the earlier scope's.
      await waitFor(browser, async () => {
        const now = await runIds();
        return now.length === 1 && !before.includes(now[0]);
      }, "the other repository's Run replaces the Runs");
    },
  ),
  // UX edges: E-MON-SCOPE-MOVE E-MON-RUNS-MOVE
  scenario<{ initialRun: string; worktreeRuns: string[] }>(
    "launch-worktrees",
    "narrows to one worktree and aggregates both at the repository boundary",
    async () => prepareMonitor(),
    async (browser, { data }) => {
      const { text } = monitor(browser);
      const { initialRun, worktreeRuns } = data;
      await waitFor(browser, async () => (await text()).includes("signoff"), "the Run tree");
      const ids = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')]
        .map((row) => /run-\S+/.exec(row.innerText)?.[0] ?? ""));
      const scope = (name: string) => browser.$(`//ul[@aria-label="Scope"]//li[span[text()="${name}"]]`);
      for (const [index, name] of ["scope-one", "scope-two"].entries()) {
        await scope(name).click();
        await waitFor(browser, async () => (await ids()).length === 1 && (await ids())[0] === worktreeRuns[index], `only ${name}'s identified Run`);
        assert.deepEqual(await ids(), [worktreeRuns[index]], "the other worktree is excluded");
      }
      await scope("repo").click();
      await waitFor(browser, async () => (await ids()).length === 3, "the repository includes its three worktrees' Runs");
      assert.deepEqual((await ids()).sort(), [initialRun, ...worktreeRuns].sort());
    },
  ),
  // UX edges: E-SCREEN-SETTINGS E-SCREEN-REVIEW E-SCREEN-MONITOR
  scenario<{ initialRun: string; worktreeRuns: string[] }>(
    "launch-screens",
    "switches screens with Ctrl+, Ctrl+2 and Ctrl+1",
    async () => prepareMonitor(),
    async (browser) => {
      const { text, active, settingsShown } = monitor(browser);
      await waitFor(browser, async () => (await text()).includes("signoff"), "the Run tree");
      await browser.$('[role="tree"]').click();
      await press(browser, [primaryModifier, ","], settingsShown, "Settings shown");
      await press(browser, [primaryModifier, "2"], async () => (await active()) === "Review", "Review active");
      await press(browser, [primaryModifier, "1"], async () => (await active()) === "Monitor", "Monitor active");
    },
  ),
  // UX edges: E-LAUNCH-MONITOR E-MON-SCOPE-MOVE
  scenario<Record<string, never>>(
    "launch-all",
    "starts on ALL with every repository's Runs and narrows to one repository",
    async () => prepareRepositories(),
    async (browser) => {
      const { text, selectedScope } = allMonitor(browser);
      await waitFor(browser, async () => (await text()).includes("signoff"), "the Run tree");
      assert.match(await selectedScope(), /^ALL/);
      await waitFor(browser, async () => /Runs \(3\)/i.test(await text()), "the Runs of all three repositories");
      const runIds = () =>
        browser.execute(() =>
          [...document.querySelectorAll('[aria-label="Runs"] [role="option"]')].map((row) => /run-\S+/.exec((row as HTMLElement).innerText)?.[0] ?? ""),
        );
      const all = await runIds();
      assert.equal(new Set(all).size, 3, "three distinct Runs");
      await browser.$('//ul[@aria-label="Scope"]//li[contains(., "third")]').click();
      await waitFor(browser, async () => /Runs \(1\)/i.test(await text()), "one repository's Run");
      const one = await runIds();
      assert.equal(one.length, 1);
      assert.ok(all.includes(one[0]), "it is one of the three");
    },
  ),
  // UX edges: E-LAUNCH-REVIEW
  scenario<Record<string, never>>(
    "launch-review-all",
    "lists the waiting Human reviews of every repository",
    async () => prepareRepositories(true),
    async (browser) => {
      const { rows, query } = review(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await waitFor(browser, async () => (await rows()).length === 3, "three waiting sign-offs");
      assert.equal(await nativeTitle(browser), "Artifactize", "the native window opened on Review is titled Artifactize");
      assert.equal(await query(), "kind:human status:waiting_human");
      const listed = await rows();
      for (const repository of ["other", "third"]) assert.ok(listed.some((row) => row.includes(repository)), `${repository} is listed`);
    },
  ),
  // UX edges: E-REVIEW-PREVIEW
  scenario<{ previews: Record<string, PreviewExpectation> }>(
    "launch-review-preview",
    "previews each selected request read-only and claims none",
    preparePreviews,
    async (browser, { fixture, data: { previews } }) => {
      const { rows, preview } = review(browser);
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      await waitFor(browser, async () => (await rows()).length === 3, "three independently identified sign-offs");
      await waitFor(browser, () => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
      const idOf = (row: string) => /run-\S+/.exec(row)![0];
      const [top, below] = await rows();
      const loaded = async (id: string, claim: string) => {
        const expected = previews[id];
        assert.ok(expected, `the fixture independently identifies ${id}`);
        return browser.execute((wantedId, title, instruction, claimant) => {
          const selected = document.querySelector('[aria-label="Requests"] [aria-selected="true"] .mono')?.textContent?.trim();
          const pane = document.querySelector('section.panel[aria-label="Preview"] .preview');
          return selected === wantedId && pane?.querySelector("h3")?.textContent?.trim() === title
            && pane.querySelector(".instruction")?.textContent?.trim() === instruction
            && (pane.querySelector(".meta")?.textContent?.trim() ?? "").startsWith(`Request ${wantedId} · WAITING_HUMAN · ${claimant} · waiting `);
        }, id, expected.title, expected.instruction, claim);
      };
      await waitFor(browser, () => loaded(idOf(top), "unclaimed"), "the first identified, loaded and unclaimed Preview");
      const first = await preview();
      assert.match(first, /release\/signoff/);
      // A Preview only shows the request: no claim, choice, field or submit control exists
      // until the request is opened, so selecting it can neither claim nor edit nor submit.
      const controls = () => browser.execute(() => {
        const shown = document.querySelector('section.panel[aria-label="Preview"] .preview');
        return {
          shown: shown !== null,
          controls: shown?.querySelectorAll('button, input, textarea, select, form, [contenteditable="true"]').length ?? -1,
          review: document.querySelector('[aria-label="Human review"]') !== null,
        };
      });
      assert.deepEqual(await controls(), { shown: true, controls: 0, review: false }, "the unclaimed Preview has no review controls");
      const pane = () => browser.$('section.panel[aria-label="Preview"]').getText();
      // The request below is claimed by someone else: its Preview says so, the first one's does not.
      assert.equal(artifactize(fixture, "request", "claim", idOf(below), "--reviewer", "bob"), 0);
      assert.doesNotMatch(await pane(), /claimed by bob/, "the first request's Preview");
      assert.match(await pane(), /unclaimed/);
      await press(browser, ["j"], () => loaded(idOf(below), "claimed by bob · read-only"), "the other identified request's loaded, read-only Preview");
      assert.deepEqual(await controls(), { shown: true, controls: 0, review: false }, "the claimed Preview has no review controls");
      await press(browser, ["k"], () => loaded(idOf(top), "unclaimed"), "the first identified request's loaded, unclaimed Preview restored");
      const claims = Object.fromEntries(requests(fixture).map((request) => [request.id, request.claim?.reviewer ?? null]));
      assert.equal(claims[idOf(top)], null, "previewing claims nothing");
      assert.equal(claims[idOf(below)], "bob");
    },
  ),
  // UX edges: E-REVIEW-SEARCH-TERMS
  scenario<{ mine: { id: string }; examples: Awaited<ReturnType<typeof addSearchExamples>> }>(
    "launch-review-terms",
    "matches every term of a search: text, request, reviewer, tag, status and mine",
    async () => {
      const prepared = await prepareRepositories(true);
      const mine = requests(prepared.fixture).find((request) => request.evalId === "release/signoff" && request.claim === null);
      assert.ok(mine);
      assert.equal(artifactize(prepared.fixture, "request", "claim", mine.id, "--reviewer", "alice"), 0);
      const examples = await addSearchExamples(prepared.fixture);
      return { ...prepared, data: { mine: { id: mine.id }, examples } };
    },
    async (browser, { data }) => {
      const { rows } = review(browser);
      const { mine, examples } = data;
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      const search = async (text: string, want: (shown: string[]) => boolean, what: string) => {
        await browser.$('[aria-label="Search requests"]').click();
        await browser.keys([primaryModifier, "a"]);
        const before = await checkpoint(browser);
        await fill(browser, '[aria-label="Search requests"]', text);
        // Distinct queries can deliberately return identical rows (reviewer:, is:mine and
        // request:). Those old rows cannot stand in for this query's completed search.
        await commandsRendered(browser, before);
        await waitFor(browser, async () => want(await rows()), `${what} (shows ${JSON.stringify(await rows())})`);
      };
      await search("kind:human reviewer:alice", (shown) => shown.length === 1 && shown[0].includes(mine.id), "only alice's claimed request");
      await search("kind:human is:mine", (shown) => shown.length === 1 && shown[0].includes(mine.id), "only my claimed request");
      await search(`request:${mine.id}`, (shown) => shown.length === 1 && shown[0].includes(mine.id), "the request by its ID");
      // Each of these terms must discriminate within the same declared request set. If
      // tag/status/free text is ignored, both its positive and negative queries fail.
      const idsOf = (shown: string[]) => shown.map((row) => /run-\S+/.exec(row)![0]).sort();
      const exact = (ids: string[]) => (shown: string[]) => JSON.stringify(idsOf(shown)) === JSON.stringify([...ids].sort());
      await search(examples.baseQuery, exact(examples.allIds), "all the independent search examples");
      for (const [kind, example, absent] of [
        ["tag", examples.tag, "tag:search:absent"],
        ["status", examples.status, "status:green"],
        ["text", examples.text, "NoSuchSearchExample"],
      ] as const) {
        await search(`${examples.baseQuery} ${example.term}`, exact([example.id]), `${kind} selects its one matching request`);
        await search(`${examples.baseQuery} ${absent}`, exact([]), `${kind} excludes every nonmatching request`);
      }
      await search(`${examples.baseQuery} ${examples.tag.term} ${examples.status.term}`, exact([]), "all terms must match the same request");
      // The claimed request's repository is the last word of its row.
      await search("kind:human reviewer:alice", (shown) => shown.length === 1, "alice's request again");
      const home = (await rows())[0].split("/").at(-1)!;
      const elsewhere = ["repo", "other", "third"].find((name) => name !== home)!;
      await search(`release kind:human repo:${home}`, (shown) => shown.length === 1 && shown[0].includes(mine.id), "free text and a repository together");
      await search(`release kind:human reviewer:alice repo:${elsewhere}`, (shown) => shown.length === 0, "no request matches every term");
      const waitingExamples = examples.allIds.filter((id) => id !== examples.status.id);
      await search("kind:human status:waiting_human", (shown) => shown.length === 5
        && waitingExamples.every((id) => idsOf(shown).includes(id)) && !idsOf(shown).includes(examples.status.id),
      "the three original sign-offs and two waiting examples, excluding the saved RED result");
    },
  ),
];
