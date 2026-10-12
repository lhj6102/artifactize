// The Review screen's keys in the real app: opening the selected request, opening it with the
// Tools focused, switching the verdict being edited, and paging through more requests than one
// page holds.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { create, createChain, type Fixture } from "./fixture.ts";
import { type Session, start } from "./driver.ts";
import { press, typeText } from "./keys.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 20000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("opening a request", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const human = () => browser.$('[aria-label="Human review"]');
  const listed = () => browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
  const gone = async () => !(await human().isExisting());
  const exists = () => human().isExisting();

  before(async () => {
    fixture = create();
    session = await start(fixture, "review");
    browser = session.browser;
    await eventually(async () => (await listed()) === 1, "the waiting sign-off");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-REVIEW-OPEN
  it("opens the selected Human review with Enter and leaves it with Esc", async () => {
    await eventually(() => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
    await press(browser, ["Enter"], exists, "the Human review");
    await press(browser, ["Escape"], gone, "back on the list");
  });

  // UX edges: E-REVIEW-OPEN-TOOLS
  it("opens it with the Tools focused from the palette", async () => {
    await press(browser, ["Control", "k"], () => browser.$('[aria-label="Command palette"][data-state="open"]').isExisting(), "the palette");
    await typeText(browser, "Tools focused");
    await press(browser, ["Enter"], exists, "the Human review");
    await eventually(() => browser.$('[aria-label="Human tools"]').isFocused(), "focus on the Tools");
    await press(browser, ["Escape"], gone, "back on the list");
  });

  // UX edges: E-HUMAN-SWITCH
  it("switches between the GREEN and the RED form while editing", async () => {
    await press(browser, ["Enter"], exists, "the Human review");
    await human().click();
    await press(browser, ["c"], async () => (await browser.execute(() => document.body.innerText)).includes("claimed by you"), "claimed");
    await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
    assert.ok(!(await browser.$("#field-reason").isExisting()), "no RED field in the GREEN form");
    await press(browser, ["Control", "r"], () => browser.$("#field-reason").isExisting(), "the RED form");
    assert.ok(!(await browser.$("#field-comment").isExisting()), "no GREEN field in the RED form");
    await press(browser, ["Control", "g"], () => browser.$("#field-comment").isExisting(), "the GREEN form again");
  });
});

describe("paging requests", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const count = () => browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
  const title = () => browser.execute(() => document.body.innerText);

  before(async () => {
    // More requests than the 100 a page holds.
    fixture = createChain(103);
    session = await start(fixture, "review");
    browser = session.browser;
    await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-REVIEW-PAGE
  it("pages to older requests with PgDn and back to newer ones with PgUp", async () => {
    // Every request, not only the waiting Human reviews: Esc clears the query.
    await browser.$('[aria-label="Search requests"]').click();
    await press(browser, ["Escape"], async () => (await browser.$('[aria-label="Search requests"]').getValue()) === "", "the query cleared");
    await press(browser, ["Enter"], () => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
    await eventually(async () => (await count()) === 100 && /page 1/.test(await title()), "the first page");
    await press(browser, ["PageDown"], async () => (await count()) === 3 && /page 2/.test(await title()), "the older page");
    await press(browser, ["PageUp"], async () => (await count()) === 100 && /page 1/.test(await title()), "the newer page");
  });
});
