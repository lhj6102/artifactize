// The review screen's request search on real state: several repositories share eval IDs. The
// launch lists its own repository's waiting sign-off; a search reaches the others, completion
// takes a typed partial key and its value, and any other request opens read-only.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { addRepository, create, type Fixture } from "./fixture.ts";
import { type Session, start } from "./driver.ts";
import { press, typeText } from "./keys.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("review search", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  /** The listed rows' text; WebKitWebDriver's element text skips these grid rows. */
  const rows = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')].map((row) => (row as HTMLElement).innerText),
    );
  const query = () => browser.execute(() => (document.querySelector('[aria-label="Search requests"]') as HTMLInputElement).value);
  const type = (text: string) => typeText(browser, text);
  const focused = (label: string) => () => browser.execute((name) => document.activeElement?.getAttribute("aria-label") === name, label);
  const holds = (text: string) => async () => (await query()) === text;
  const readOnly = async () => (await browser.execute(() => document.body.innerText)).includes("· read-only");
  const suggestions = () =>
    browser.execute(() => [...document.querySelectorAll('[aria-label="Suggestions"] [role="option"]')].map((option) => option.textContent?.trim()));

  before(async () => {
    fixture = create();
    addRepository(fixture, "other");
    addRepository(fixture, "third");
    session = await start(fixture, "review");
    browser = session.browser;
    await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-LAUNCH-REVIEW
  it("starts on the launch repository's waiting sign-off", async () => {
    await eventually(async () => (await rows()).length === 1, "one waiting row");
    assert.match((await rows())[0], /release\/signoff/);
    assert.match(await query(), /^kind:human status:waiting_human repo:/);
  });

  // UX edges: E-REVIEW-SEARCH-FOCUS E-REVIEW-SEARCH-TERMS E-REVIEW-SUGGEST E-REVIEW-SUGGEST-CLOSE E-REVIEW-SEARCH-CLEAR
  it("filters every repository's requests and takes suggestions for a partial key and value", async () => {
    await press(browser, ["/"], focused("Search requests"), "focus in the search");
    // Esc clears the query: every request of every repository.
    await press(browser, ["Escape"], holds(""), "the query cleared");
    await eventually(async () => (await rows()).length === 3 * 3, "every request of the three repositories");

    await type("kind:hu");
    await eventually(async () => (await suggestions()).includes("kind:human"), "the kind suggestion");
    // Esc closes the suggestions and keeps the query; the next Esc clears it.
    await press(browser, ["Escape"], async () => (await suggestions()).length === 0, "the suggestions closed");
    assert.equal(await query(), "kind:hu", "the query is kept");
    await press(browser, ["Escape"], holds(""), "the query cleared");
    await type("kind:hu");
    await eventually(async () => (await suggestions()).includes("kind:human"), "the kind suggestion again");
    await press(browser, ["Tab"], holds("kind:human "), "the kind taken");
    await eventually(async () => (await rows()).length === 3, "the three sign-offs");

    await type("rep");
    await eventually(async () => (await suggestions()).includes("repo:"), "the repo key");
    const chosen = () =>
      browser.execute(() => document.querySelector('[aria-label="Suggestions"] [aria-selected="true"]')?.textContent?.trim() ?? null);
    await press(browser, ["ArrowDown"], async () => (await chosen()) === "repo:", "the repo key chosen");
    await press(browser, ["Enter"], holds("kind:human repo:"), "the repo key taken");
    // A taken key offers the listed repositories' folders.
    await eventually(async () => (await suggestions()).includes("repo:other"), "the repository values");
    await type("oth");
    await eventually(async () => (await suggestions()).includes("repo:other"), "the other repository");
    await press(browser, ["Tab"], holds("kind:human repo:other "), "the repository taken");
    await eventually(async () => {
      const shown = await rows();
      return shown.length === 1 && /release\/signoff/.test(shown[0]) && /other/.test(shown[0]);
    }, "the other repository's sign-off");
  });

  // UX edges: E-REVIEW-READONLY E-REVIEW-SEARCH-LEAVE
  it("opens a request that is not a Human review read-only", async () => {
    // Esc clears the query; a new search, then Enter leaves for the list.
    await press(browser, ["Escape"], holds(""), "the query cleared");
    // A space ends the term and closes the suggestions, so Enter leaves the field.
    await type("kind:runtime eval:docs repo:third ");
    await press(browser, ["Enter"], focused("Requests"), "focus on the list");
    // An empty query's Esc leaves the field for the list as well.
    await press(browser, ["/"], focused("Search requests"), "focus in the search again");
    await press(browser, ["Escape"], holds(""), "the query cleared again");
    await press(browser, ["Escape"], focused("Requests"), "Esc left for the list");
    await press(browser, ["/"], focused("Search requests"), "focus in the search a third time");
    await type("kind:runtime eval:docs repo:third ");
    await press(browser, ["Enter"], focused("Requests"), "focus on the list again");
    await eventually(async () => {
      const shown = await rows();
      return shown.length === 1 && shown[0].includes("docs/lint") && shown[0].includes("third");
    }, "the third repository's docs eval");
    await press(browser, ["Enter"], readOnly, "the read-only Detail");
    assert.ok(!(await browser.$('[aria-label="Human review"]').isExisting()), "no Human review");
    await press(browser, ["Escape"], async () => !(await readOnly()), "back on the list");
  });
});
