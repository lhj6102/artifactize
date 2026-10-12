// The review screen's request search on real state: several repositories share eval IDs. The
// launch lists its own repository's waiting sign-off; a search reaches the others, completion
// takes a typed partial key and its value, and any other request opens read-only.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { addRepository, create, type Fixture } from "./fixture.ts";
import { type Session, start } from "./driver.ts";

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
  /**
   * Type `text` one key at a time: WebKitWebDriver drops some keys of a long burst while the
   * list re-renders, which a person's typing never does.
   */
  const type = async (text: string) => {
    for (const key of text) await browser.keys([key]);
  };
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

  it("starts on the launch repository's waiting sign-off", async () => {
    await eventually(async () => (await rows()).length === 1, "one waiting row");
    assert.match((await rows())[0], /release\/signoff/);
    assert.match(await query(), /^kind:human status:waiting_human repo:/);
  });

  it("filters every repository's requests and takes suggestions for a partial key and value", async () => {
    await browser.keys(["/"]);
    await eventually(
      () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Search requests"),
      "focus in the search",
    );
    // Esc clears the query: every request of every repository.
    await browser.keys(["Escape"]);
    await eventually(async () => (await rows()).length === 3 * 3, "every request of the three repositories");

    await type("kind:hu");
    await eventually(async () => (await suggestions()).includes("kind:human"), "the kind suggestion");
    await browser.keys(["Tab"]);
    assert.equal(await query(), "kind:human ");
    await eventually(async () => (await rows()).length === 3, "the three sign-offs");

    await type("rep");
    await eventually(async () => (await suggestions()).includes("repo:"), "the repo key");
    await browser.keys(["ArrowDown", "Enter"]);
    assert.equal(await query(), "kind:human repo:");
    // A taken key offers the listed repositories' folders.
    await eventually(async () => (await suggestions()).includes("repo:other"), "the repository values");
    await type("oth");
    await browser.keys(["Tab"]);
    assert.equal(await query(), "kind:human repo:other ");
    await eventually(async () => (await rows()).length === 1, "the other repository's sign-off");
    const [row] = await rows();
    assert.match(row, /release\/signoff/);
    assert.match(row, /other/);
  });

  it("opens a request that is not a Human review read-only", async () => {
    // Esc clears the query; a new search, then Enter leaves for the list.
    await browser.keys(["Escape"]);
    // A space ends the term and closes the suggestions, so Enter leaves the field.
    await type("kind:runtime eval:docs repo:third ");
    await browser.keys(["Enter"]);
    await eventually(async () => {
      const shown = await rows();
      return shown.length === 1 && shown[0].includes("docs/lint") && shown[0].includes("third");
    }, "the third repository's docs eval");
    await eventually(
      () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Requests"),
      "focus on the list",
    );
    await browser.keys(["Enter"]);
    await eventually(
      async () => (await browser.execute(() => document.body.innerText)).includes("· read-only"),
      "the read-only Detail",
    );
    assert.ok(!(await browser.$('[aria-label="Human review"]').isExisting()), "no Human review");
    await browser.keys(["Escape"]);
    await eventually(
      async () => !(await browser.execute(() => document.body.innerText)).includes("· read-only"),
      "back on the list",
    );
  });
});
