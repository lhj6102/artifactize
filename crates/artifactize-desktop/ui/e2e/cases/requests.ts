// The Review screen's keys in the real app: opening the selected request, opening it with the
// Tools focused, switching the verdict being edited, and paging through more requests than one
// page holds.
import assert from "node:assert/strict";
import { artifactize, create, createPagedRequests, signoff } from "../fixture.ts";
import { afterRender, press, typeText } from "../keys.ts";
import { checkpoint } from "../commands.ts";

import { scenario, type Scenario } from "../scenario.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 20000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

async function prepareReview() {
  const fixture = await create();
  return { fixture, args: ["review"], data: {} };
}

function opening(browser: WebdriverIO.Browser) {
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  const listed = () => browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
  const gone = async () => !(await human().isExisting());
  const exists = () => human().isExisting();
  return { human, listed, gone, exists };
}

function paging(browser: WebdriverIO.Browser) {
  const count = () => browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
  const title = () => browser.execute(() => document.body.innerText);
  return { count, title };
}

function claim(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  return { text, human };
}

function submission(browser: WebdriverIO.Browser) {
  const text = () => browser.execute(() => document.body.innerText);
  // The surrounding Monitor panel shares this label but does not own the Human keys.
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  /** A key typed while an input method composes: the page marks it so. */
  const composing = (key: string) =>
    browser.execute((name) => {
      document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: name, isComposing: true, bubbles: true }));
    }, key);
  return { text, human, composing };
}

export const scenarios: Scenario[] = [
  // UX edges: E-REVIEW-OPEN
  scenario<Record<string, never>>(
    "requests-open",
    "opens the selected Human review with Enter and leaves it with Esc",
    async () => prepareReview(),
    async (browser) => {
      const { listed, gone, exists } = opening(browser);
      await waitFor(browser, async () => (await listed()) === 1, "the waiting sign-off");
      await waitFor(browser, () => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
      await press(browser, ["Enter"], exists, "the Human review");
      await press(browser, ["Escape"], gone, "back on the list");
    },
  ),
  // UX edges: E-REVIEW-OPEN-TOOLS
  scenario<Record<string, never>>(
    "requests-tools",
    "opens it with the Tools focused from the palette",
    async () => prepareReview(),
    async (browser) => {
      const { listed, gone, exists } = opening(browser);
      await waitFor(browser, async () => (await listed()) === 1, "the waiting sign-off");
      await press(browser, ["Control", "k"], () => browser.$('[aria-label="Command palette"][data-state="open"]').isExisting(), "the palette");
      await typeText(browser, "Tools focused");
      await press(browser, ["Enter"], exists, "the Human review");
      await waitFor(browser, () => browser.$('[aria-label="Human tools"]').isFocused(), "focus on the Tools");
      await press(browser, ["Escape"], gone, "back on the list");
    },
  ),
  // UX edges: E-HUMAN-SWITCH
  scenario<Record<string, never>>(
    "requests-verdict-switch",
    "switches between the GREEN and the RED form while editing",
    async () => prepareReview(),
    async (browser) => {
      const { human, listed, exists } = opening(browser);
      await waitFor(browser, async () => (await listed()) === 1, "the waiting sign-off");
      await press(browser, ["Enter"], exists, "the Human review");
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], async () => (await browser.execute(() => document.body.innerText)).includes("claimed by you"), "claimed");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "no RED field in the GREEN form");
      await browser.$("#field-comment").setValue("GREEN draft stays");
      // An idempotent verdict leaves the same field visible: require the real chord's
      // defaultPrevented flag after the app handler, not only that already-true field state.
      await press(browser, ["Control", "g"], () => browser.$("#field-comment").isExisting(), "Ctrl+G preserves GREEN", { handled: true });
      assert.equal(await browser.$("#field-comment").getValue(), "GREEN draft stays");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "Ctrl+G kept the GREEN schema");
      await press(browser, ["Control", "r"], () => browser.$("#field-reason").isExisting(), "the RED form");
      assert.ok(!(await browser.$("#field-comment").isExisting()), "no GREEN field in the RED form");
      await browser.$("#field-reason").setValue("RED draft stays");
      await press(browser, ["Control", "r"], () => browser.$("#field-reason").isExisting(), "Ctrl+R preserves RED", { handled: true });
      assert.equal(await browser.$("#field-reason").getValue(), "RED draft stays");
      assert.ok(!(await browser.$("#field-comment").isExisting()), "Ctrl+R kept the RED schema");
      await press(browser, ["Control", "g"], () => browser.$("#field-comment").isExisting(), "the GREEN form again");
      assert.equal(await browser.$("#field-comment").getValue(), "GREEN draft stays");
    },
  ),
  // UX edges: E-REVIEW-PAGE
  scenario<{ expected: string[] }>(
    "requests-paging",
    "pages to older requests with PgDn and back to newer ones with PgUp",
    async () => {
      // More requests than the 100 a page holds.
      const { fixture, expected } = createPagedRequests();
      return { fixture, args: ["review"], data: { expected } };
    },
    async (browser, { data }) => {
      const { count, title } = paging(browser);
      const { expected } = data;
      await browser.$('[aria-label="Search requests"]').waitForExist({ timeout: 15000 });
      // Every request, not only the waiting Human reviews: Esc clears the query.
      await browser.$('[aria-label="Search requests"]').click();
      await press(browser, ["Escape"], async () => (await browser.$('[aria-label="Search requests"]').getValue()) === "", "the query cleared");
      await press(browser, ["Enter"], () => browser.$('[aria-label="Requests"]').isFocused(), "focus on the list");
      await waitFor(browser, async () => (await count()) === 100 && /page 1/.test(await title()), "the first page");
      const identities = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Requests"] [role="option"] .mono')].map((cell) => cell.innerText));
      assert.deepEqual(await identities(), expected.slice(0, 100), "the newest batch precedes the older batch");
      await press(browser, ["PageDown"], async () => (await count()) === 3 && /page 2/.test(await title()), "the older page");
      assert.deepEqual(await identities(), expected.slice(100), "older request identities and order");
      await press(browser, ["PageUp"], async () => (await count()) === 100 && /page 1/.test(await title()), "the newer page");
      assert.deepEqual(await identities(), expected.slice(0, 100), "paging back restores the identical newer order");
    },
  ),
  // UX edges: E-HUMAN-CLAIMED-ELSEWHERE E-HUMAN-RELEASED-ELSEWHERE
  scenario<Record<string, never>>(
    "requests-claim-elsewhere",
    "refuses a claim here while it is held, and offers it again once released",
    async () => prepareReview(),
    async (browser, { fixture }) => {
      const { text, human } = claim(browser);
      await waitFor(browser, async () => (await browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length)) === 1, "the sign-off");
      const id = signoff(fixture).id;
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => (await text()).includes("claimed by bob"), "bob's claim shown");
      await press(browser, ["Enter"], () => human().isExisting(), "the Human review");
      await human().$("button*=Claim").waitForExist({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], () => browser.execute(() =>
        [...document.querySelectorAll('[aria-label="Human review"] .notice.error')].some((notice) => /bob/.test(notice.textContent ?? "")),
      ), "this opened review's claim refusal names bob");
      assert.equal(signoff(fixture).claim?.reviewer, "bob", "the claim stays bob's");
      assert.doesNotMatch(await text(), /claimed by you/);
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => signoff(fixture).claim === null, "released");
      await waitFor(browser, async () => !/claimed by bob/.test(await text()), "the release shown");
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed here after the release");
    },
  ),
  // UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  scenario<Record<string, never>>(
    "requests-green-composition",
    "claims, fills the schema's form with Korean text composed in the field, and submits GREEN",
    async () => ({ fixture: await create(), data: {} }),
    async (browser, { fixture }) => {
      const { text, human, composing } = submission(browser);
      await waitFor(browser, async () => (await text()).includes("signoff"), "the Run tree");
      await browser.$('[role="tree"]').click();
      await browser.$('//li[@role="treeitem"][@aria-level="2"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "signoff")]]').click();
      await press(browser, ["Enter"], () => human().isExisting(), "the Human review");
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
      // The window shows the claim before a verdict can be chosen.
      await waitFor(browser, async () => (await text()).includes("claimed by you"), "the claim shown");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form of the pass schema");
      assert.ok(await browser.$("#field-approved").isExisting(), "the schema's approved field");
      const comment = browser.$("#field-comment");
      await comment.click();
      // Composition: shortcut letters, chords and Esc belong to the input method. Observe
      // dispatch, not only a still-unchanged database while an accidental command is pending.
      const writes = ["humanForm", "humanClaim", "humanRelease", "humanRun", "humanSubmit", "humanCancel", "humanJson"];
      const before = (await checkpoint(browser)).activity;
      assert.ok(before.commands.humanClaim?.completed > 0, "the same read-only activity probe observes this case's successful claim");
      await browser.execute(() => document.activeElement?.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true })));
      for (const key of ["g", "r", "q", "u", "c", "Escape"]) await composing(key);
      await browser.execute(() => {
        for (const key of ["g", "r", "u", "s", "e"]) {
          document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: true, isComposing: true, bubbles: true }));
        }
      });
      await afterRender(browser);
      const after = (await checkpoint(browser)).activity;
      for (const command of writes) assert.equal(after.commands[command]?.started ?? 0, before.commands[command]?.started ?? 0, `composition dispatches no ${command}`);
      assert.ok(await comment.isExisting(), "the form stays open");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "nothing was released");
      assert.equal(signoff(fixture).status, "WAITING_HUMAN");
      // The composition commits: the field holds the Korean text.
      await browser.execute(() => {
        const field = document.querySelector<HTMLInputElement>("#field-comment")!;
        field.value = "안녕";
        field.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: "안녕" }));
        field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "안녕" }));
      });
      assert.equal(await comment.getValue(), "안녕");
      await press(browser, ["Control", "s"], async () => signoff(fixture).status === "GREEN", "GREEN saved");
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "안녕" });
      await waitFor(browser, async () => /completed result/i.test(await text()), "the completed result stays open");
    },
  ),
];
