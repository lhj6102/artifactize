// The Review screen's keys in the real app: opening the selected request, opening it with the
// Tools focused, switching the verdict being edited, and paging through more requests than one
// page holds.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";
import { artifactize, create, createPagedRequests, signoff } from "../fixture.ts";
import { fill, press } from "../keys.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { reviewing } from "../human-review.ts";

import { scenario, type Scenario } from "../scenario.ts";
import { nativeIme, primaryModifier } from "../platform.ts";
import { graphShown, openArtifact, openTab } from "../graph-view.ts";

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 20000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

async function prepareReview() {
  const fixture = await create();
  return { fixture, args: ["review"], data: {} };
}

function opening(browser: WebdriverIO.Browser, requestId: string) {
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  const listed = () => browser.execute(() => document.querySelectorAll('[aria-label="Requests"] [role="option"]').length);
  const gone = async () => !(await human().isExisting());
  const exists = () => browser.execute((id) => {
    const opened = document.querySelector('[aria-label="Human review"][tabindex="0"]');
    return opened?.querySelector("h3")?.textContent?.trim() === "release/signoff · A person approves the release"
      && opened.querySelector(".instruction .text")?.textContent?.trim() === "Read the release notes and approve them."
      && opened.querySelector(".stage")?.textContent?.trim() === "CLAIM"
      && (opened.querySelector(".meta")?.textContent?.trim() ?? "").startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `);
  }, requestId);
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
  return { text, human };
}

export const scenarios: Scenario[] = [
  // UX edges: E-REVIEW-OPEN
  scenario<Record<string, never>>(
    "requests-open",
    "opens the selected Human review with Enter and leaves it with Esc",
    async () => prepareReview(),
    async (browser, { fixture }) => {
      const { listed, gone, exists } = opening(browser, signoff(fixture).id);
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
    async (browser, { fixture }) => {
      const { listed, gone, exists } = opening(browser, signoff(fixture).id);
      await waitFor(browser, async () => (await listed()) === 1, "the waiting sign-off");
      await press(browser, [primaryModifier, "k"], () => browser.$('[aria-label="Command palette"][data-state="open"]').isExisting(), "the palette");
      await choosePaletteCommand(browser, "Tools focused", "Open the selected request with Tools focused");
      await press(browser, ["Enter"], exists, "the Human review");
      await waitFor(browser, () => browser.$('section[aria-label="Tools"]').isFocused(), "focus on the Tools");
      await press(browser, ["Escape"], gone, "back on the list");
    },
  ),
  // UX edges: E-HUMAN-SWITCH
  scenario<Record<string, never>>(
    "requests-verdict-switch",
    "switches between the GREEN and the RED form while editing",
    async () => prepareReview(),
    async (browser, { fixture }) => {
      const { human, listed, exists } = opening(browser, signoff(fixture).id);
      await waitFor(browser, async () => (await listed()) === 1, "the waiting sign-off");
      await press(browser, ["Enter"], exists, "the Human review");
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], async () => (await browser.execute(() => document.body.innerText)).includes("claimed by you") && await reviewing(browser), "claimed");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "no RED field in the GREEN form");
      await fill(browser, "#field-comment", "GREEN draft stays");
      // An idempotent verdict leaves the same field visible: require the real chord's
      // defaultPrevented flag after the app handler, not only that already-true field state.
      await press(browser, [primaryModifier, "g"], () => browser.$("#field-comment").isExisting(), "Ctrl+G preserves GREEN", { handled: true });
      assert.equal(await browser.$("#field-comment").getValue(), "GREEN draft stays");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "Ctrl+G kept the GREEN schema");
      await press(browser, [primaryModifier, "r"], () => browser.$("#field-reason").isExisting(), "the RED form");
      assert.ok(!(await browser.$("#field-comment").isExisting()), "no GREEN field in the RED form");
      await fill(browser, "#field-reason", "RED draft stays");
      await press(browser, [primaryModifier, "r"], () => browser.$("#field-reason").isExisting(), "Ctrl+R preserves RED", { handled: true });
      assert.equal(await browser.$("#field-reason").getValue(), "RED draft stays");
      assert.ok(!(await browser.$("#field-comment").isExisting()), "Ctrl+R kept the RED schema");
      await press(browser, [primaryModifier, "g"], () => browser.$("#field-comment").isExisting(), "the GREEN form again");
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
      const identities = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Requests"] [role="option"] .mono')].map((cell) => cell.innerText));
      const page = async (number: number, ids: string[]) =>
        (await count()) === ids.length && new RegExp(`page ${number}`).test(await title()) && isDeepStrictEqual(await identities(), ids);
      await waitFor(browser, () => page(1, expected.slice(0, 100)), "the first page's exact identities");
      assert.deepEqual(await identities(), expected.slice(0, 100), "the newest batch precedes the older batch");
      await press(browser, ["PageDown"], () => page(2, expected.slice(100)), "the older page's exact identities");
      assert.deepEqual(await identities(), expected.slice(100), "older request identities and order");
      await press(browser, ["PageUp"], () => page(1, expected.slice(0, 100)), "the newer page's exact identities");
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
      // Taken: the review says who holds it and offers no Claim button.
      await waitFor(browser, async () => (await human().getText()).includes("Claimed by bob. Read-only until released.")
        && !(await human().$("button*=Claim").isExisting()), "bob's claim shown as read-only");
      await human().click();
      await press(browser, ["c"], () => browser.execute(() =>
        [...document.querySelectorAll('[aria-label="Human review"] .notice.error')].some((notice) => /bob/.test(notice.textContent ?? "")),
      ), "this opened review's claim refusal names bob");
      assert.equal(signoff(fixture).claim?.reviewer, "bob", "the claim stays bob's");
      assert.doesNotMatch(await text(), /claimed by you/);
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => signoff(fixture).claim === null, "released");
      await waitFor(browser, async () =>
        (await human().$(".meta").getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `)
        && await human().$("button*=Claim").isEnabled(), "the loaded review shows the release and offers Claim again");
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed here after the release");
      await waitFor(browser, async () => (await human().$(".meta").getText()).includes("claimed by you (alice)"), "this review displays the new claim");
    },
  ),
  // UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  scenario<Record<string, never>>(
    "requests-green-composition",
    "claims, composes Korean through the engine and guards page shortcuts before submitting GREEN",
    async () => ({ fixture: await create(), data: {} }),
    async (browser, { fixture }) => {
      const { text, human } = submission(browser);
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await human().waitForExist({ timeout: 15000, timeoutMsg: "the Human review" });
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
      // The window shows the claim before a verdict can be chosen.
      await waitFor(browser, async () => (await text()).includes("claimed by you") && await reviewing(browser), "the claim shown");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form of the pass schema");
      assert.equal(await browser.$("#field-approved").isExisting(), false, "the schema's approved constant is filled in, not shown");
      const comment = browser.$("#field-comment");
      await comment.click();
      type ImeEvent = { type: string; code: string; composing: boolean; keyCode: number; trusted: boolean };
      await browser.execute(() => {
        const log: { type: string; code: string; composing: boolean; keyCode: number; trusted: boolean }[] = [];
        (window as unknown as { imeEvents: typeof log }).imeEvents = log;
        for (const type of ["compositionstart", "compositionupdate", "compositionend", "keydown"]) {
          window.addEventListener(type, (event) => {
            const key = event as KeyboardEvent;
            log.push({ type, code: key.code ?? "", composing: key.isComposing ?? false, keyCode: key.keyCode ?? 0, trusted: event.isTrusted });
          }, { capture: true });
        }
      });
      const events = () => browser.execute(() => (window as unknown as { imeEvents: ImeEvent[] }).imeEvents);
      const checked = ["humanForm", "humanClaim", "humanSubmit"];
      const before = (await checkpoint(browser)).activity;
      assert.ok(before.commands.humanClaim?.completed > 0, "the claim counter observed this successful claim");
      assert.ok(before.commands.humanForm?.completed > 0, "the form counter observed this successful GREEN choice");
      await nativeIme(browser, { type: "set", text: "안" });
      await waitFor(browser, async () => (await events()).some((event) => event.type === "compositionstart" && event.trusted), "a trusted native composition started");
      await nativeIme(browser, { type: "set", text: "안녕" });
      await waitFor(browser, async () => await comment.getValue() === "안녕" && (await events()).some((event) =>
        event.type === "compositionupdate" && event.trusted), "the engine updated the native Korean composition");
      // E2E-INPUT: these fields are the app's own shortcut guard, not an OS input test.
      // Exercise each guard independently while the real native composition remains active.
      for (const [composing, keyCode] of [[true, 0], [false, 229]] as const) {
        for (const [key, primary] of [["g", false], ["r", false], ["q", false], ["u", false], ["c", false], ["Escape", false], ["g", true], ["r", true], ["s", true]] as const) {
          const received = await browser.execute((input) => {
            const target = document.activeElement;
            if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) throw new Error("the composition field lost focus");
            const event = new KeyboardEvent("keydown", {
              key: input.key, code: input.key === "Escape" ? "Escape" : `Key${input.key.toUpperCase()}`,
              bubbles: true, cancelable: true, isComposing: input.composing, keyCode: input.keyCode,
              ctrlKey: input.primary && input.modifier === "Control", metaKey: input.primary && input.modifier === "Meta",
            });
            target.dispatchEvent(event);
            return { composing: event.isComposing, keyCode: event.keyCode, key: event.key };
          }, { key, primary, modifier: primaryModifier, composing, keyCode });
          assert.deepEqual(received, { composing, keyCode, key }, "the page received the guard fields it actually reads");
        }
      }
      assert.ok(!(await events()).some((event) => event.type === "compositionend"), "native composition stayed active through every guarded shortcut");
      await commandsRendered(browser);
      const after = (await checkpoint(browser)).activity;
      for (const name of checked) assert.equal(after.commands[name]?.started ?? 0, before.commands[name]?.started ?? 0, `native composition dispatches no ${name}`);
      assert.ok(await comment.isExisting(), "the GREEN form stays open");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "composition did not switch the verdict");
      assert.ok(!(await browser.$('[role="alertdialog"]').isExisting()), "composition did not leave or quit");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "composition did not release the claim");
      assert.equal(signoff(fixture).status, "WAITING_HUMAN", "composition submitted nothing");
      await nativeIme(browser, { type: "commit", text: "안녕" });
      try {
        // The engine's commit API can mark compositionend untrusted. Native
        // composing start/update/key events above must be trusted; completion is
        // witnessed by compositionend, the exact committed text and real submit.
        await waitFor(browser, async () => await comment.getValue() === "안녕" && (await events()).some((event) => event.type === "compositionend"), "the native input method committed Korean text");
      } catch (error) { throw new Error(`Native commit: ${JSON.stringify({ value: await comment.getValue(), events: await events() })}`, { cause: error }); }
      await commandsRendered(browser);
      await press(browser, [primaryModifier, "s"], async () => signoff(fixture).status === "GREEN", "GREEN saved");
      await waitFor(browser, async () => ((await checkpoint(browser)).activity.commands.humanSubmit?.completed ?? 0) > (before.commands.humanSubmit?.completed ?? 0), "the submit counter observes this real successful submission");
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "안녕" });
      await waitFor(browser, async () => /completed result/i.test(await text()), "the completed result stays open");
    },
  ),
];
