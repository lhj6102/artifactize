// Launching and the window: the Monitor's catalog of
// repositories, scopes, Runs and verdicts, the screen shortcuts, and the Review screen that
// starts on the waiting sign-offs of every repository.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { addRepository, artifactize, create, type Fixture, requests } from "./fixture.ts";
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

describe("launching the Monitor", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);
  const scopes = () =>
    browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')].map((row) => (row as HTMLElement).innerText.replace(/\s+/g, " ").trim()));
  const active = () => browser.execute(() => document.querySelector('[role="tab"][data-state="active"]')?.textContent ?? "");
  const settingsShown = () => browser.execute(() => !!document.querySelector(".settings h1") && (document.querySelector(".settings") as HTMLElement).offsetParent !== null);

  before(async () => {
    fixture = create();
    addRepository(fixture, "other");
    addRepository(fixture, "third");
    session = await start(fixture, "--repo", fixture.repo);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-MON-SCOPE-MOVE E-MON-RUNS-MOVE
  it("catalogs the repositories, their Runs and the verdicts of a Run's tree", async () => {
    const entries = await scopes();
    assert.ok(entries.some((entry) => entry.startsWith("ALL")), `ALL is listed: ${entries.join(" | ")}`);
    assert.ok(entries.some((entry) => entry.includes("other")), "the other repository is listed");
    assert.ok(entries.some((entry) => entry.includes("third")), "the third repository is listed");
    // The selected repository's Run: its Artifacts with their verdicts.
    const tree = await browser.$('[role="tree"]').getText();
    for (const name of ["release", "ship", "docs"]) assert.match(tree, new RegExp(name));
    assert.match(await text(), /WAITING_HUMAN — waiting for a Human submission/);
    // Another scope lists its own Runs.
    const runIds = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[aria-label="Runs"] [role="option"]')].map((row) => /run-\S+/.exec((row as HTMLElement).innerText)?.[0] ?? ""),
      );
    const before = await runIds();
    const other = await browser.$('//ul[@aria-label="Scope"]//li[contains(., "other")]');
    await other.click();
    await eventually(async () => (await browser.$('nav[aria-label="Breadcrumb"]').getText()).includes("other"), "the other scope selected");
    await eventually(async () => /Runs \(1\)/i.test(await text()), "its one Run");
    // The Runs pane now lists that repository's own Run, not the earlier scope's.
    await eventually(async () => {
      const now = await runIds();
      return now.length === 1 && !before.includes(now[0]);
    }, "the other repository's Run replaces the Runs");
  });

  // UX edges: E-SCREEN-SETTINGS E-SCREEN-REVIEW E-SCREEN-MONITOR
  it("switches screens with Ctrl+, Ctrl+2 and Ctrl+1", async () => {
    await browser.$('[role="tree"]').click();
    await press(browser, ["Control", ","], settingsShown, "Settings shown");
    await press(browser, ["Control", "2"], async () => (await active()) === "Review", "Review active");
    await press(browser, ["Control", "1"], async () => (await active()) === "Monitor", "Monitor active");
  });
});

describe("launching the Monitor for every repository", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);
  const selectedScope = () =>
    browser.execute(() => (document.querySelector('[aria-label="Scope"] [aria-selected="true"]') as HTMLElement | null)?.innerText ?? "");

  before(async () => {
    fixture = create();
    addRepository(fixture, "other");
    addRepository(fixture, "third");
    // A plain launch, no --repo and no --all: the Monitor starts on everything in the state.
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-LAUNCH-MONITOR E-MON-SCOPE-MOVE
  it("starts on ALL with every repository's Runs and narrows to one repository", async () => {
    assert.match(await selectedScope(), /^ALL/);
    await eventually(async () => /Runs \(3\)/i.test(await text()), "the Runs of all three repositories");
    const runIds = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[aria-label="Runs"] [role="option"]')].map((row) => /run-\S+/.exec((row as HTMLElement).innerText)?.[0] ?? ""),
      );
    const all = await runIds();
    assert.equal(new Set(all).size, 3, "three distinct Runs");
    await browser.$('//ul[@aria-label="Scope"]//li[contains(., "third")]').click();
    await eventually(async () => /Runs \(1\)/i.test(await text()), "one repository's Run");
    const one = await runIds();
    assert.equal(one.length, 1);
    assert.ok(all.includes(one[0]), "it is one of the three");
  });
});

describe("launching the Review screen for every repository", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const rows = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')].map((row) => (row as HTMLElement).innerText.replace(/\s+/g, " ").trim()),
    );
  const query = () => browser.execute(() => (document.querySelector('[aria-label="Search requests"]') as HTMLInputElement).value);
  const preview = () => browser.execute(() => [...document.querySelectorAll("section.panel h3")].at(-1)?.textContent?.trim() ?? "");

  before(async () => {
    fixture = create();
    addRepository(fixture, "other");
    addRepository(fixture, "third");
    // A plain launch, no --repo and no --all: the search is not narrowed to one repository.
    session = await start(fixture, "review");
    browser = session.browser;
    await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-LAUNCH-REVIEW
  it("lists the waiting Human reviews of every repository", async () => {
    await eventually(async () => (await rows()).length === 3, "three waiting sign-offs");
    assert.equal(await query(), "kind:human status:waiting_human");
    const listed = await rows();
    for (const repository of ["other", "third"]) assert.ok(listed.some((row) => row.includes(repository)), `${repository} is listed`);
  });

  // UX edges: E-REVIEW-PREVIEW
  it("previews each selected request read-only and claims none", async () => {
    await eventually(() => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
    const first = await preview();
    assert.match(first, /release\/signoff/);
    /** The Preview pane's text: the last pane of the window. */
    const pane = () => browser.execute(() => (document.querySelectorAll("section.panel")[5] as HTMLElement).innerText);
    const idOf = (row: string) => /run-\S+/.exec(row)![0];
    const [top, below] = await rows();
    // The request below is claimed by someone else: its Preview says so, the first one's does not.
    assert.equal(artifactize(fixture, "request", "claim", idOf(below), "--reviewer", "bob"), 0);
    assert.doesNotMatch(await pane(), /claimed by bob/, "the first request's Preview");
    assert.match(await pane(), /unclaimed/);
    await press(browser, ["j"], async () => (await browser.$('[aria-label="Requests"] [aria-selected="true"]').getText()) !== top, "another row selected");
    await eventually(async () => /claimed by bob · read-only/.test(await pane()), "the Preview of the other request, read-only");
    await press(browser, ["k"], async () => !/claimed by bob/.test(await pane()), "back on the first request's Preview");
    const claims = Object.fromEntries(requests(fixture).map((request) => [request.id, request.claim?.reviewer ?? null]));
    assert.equal(claims[idOf(top)], null, "previewing claims nothing");
    assert.equal(claims[idOf(below)], "bob");
  });

  // UX edges: E-REVIEW-SEARCH-TERMS
  it("matches every term of a search: text, request, reviewer, tag, status and mine", async () => {
    // The first repository's sign-off is claimed by alice, so the claim terms tell them apart.
    const mine = requests(fixture).find((request) => request.evalId === "release/signoff" && request.claim === null);
    assert.ok(mine);
    assert.equal(artifactize(fixture, "request", "claim", mine.id, "--reviewer", "alice"), 0);
    const search = async (text: string, want: (shown: string[]) => boolean, what: string) => {
      await browser.$('[aria-label="Search requests"]').click();
      await browser.keys(["Control", "a"]);
      await browser.$('[aria-label="Search requests"]').setValue(text);
      await eventually(async () => want(await rows()), `${what} (shows ${JSON.stringify(await rows())})`);
    };
    await search("kind:human reviewer:alice", (shown) => shown.length === 1 && shown[0].includes(mine.id), "only alice's claimed request");
    await search("kind:human is:mine", (shown) => shown.length === 1 && shown[0].includes(mine.id), "only my claimed request");
    await search(`request:${mine.id}`, (shown) => shown.length === 1 && shown[0].includes(mine.id), "the request by its ID");
    await search("kind:human tag:type:release", (shown) => shown.length === 3, "all three, tagged type:release");
    // The claimed request's repository is the last word of its row.
    await search("kind:human reviewer:alice", (shown) => shown.length === 1, "alice's request again");
    const home = (await rows())[0].split("/").at(-1)!;
    const elsewhere = ["repo", "other", "third"].find((name) => name !== home)!;
    await search(`release kind:human repo:${home}`, (shown) => shown.length === 1 && shown[0].includes(mine.id), "free text and a repository together");
    await search(`release kind:human reviewer:alice repo:${elsewhere}`, (shown) => shown.length === 0, "no request matches every term");
    await search("kind:human status:waiting_human", (shown) => shown.length === 3, "every waiting sign-off again");
  });
});
