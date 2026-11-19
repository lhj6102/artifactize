// A Human review in the real app, opened in the Main area at launch or in a Run's Detail:
// switching the verdict being edited, a claim held elsewhere, and Korean composed through the
// engine before a GREEN submission.
import assert from "node:assert/strict";
import { artifactize, create, signoff } from "../fixture.ts";
import { fill, press } from "../keys.ts";
import { dispatchInput } from "../input.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { chooseVerdict, requestState, showDebug, reviewing } from "../human-review.ts";
import { MAIN, mainTitle } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";
import { nativeInput } from "../input.ts";
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
  /** The review open in the Main area, unclaimed, its title in the header row. */
  const exists = async () => (await browser.execute((main) => {
    const opened = document.querySelector('[aria-label="Human review"][tabindex="0"]');
    return document.querySelector(`${main} > header .headline .title`)?.textContent?.trim() === "release/signoff · A person approves the release"
      && opened?.querySelector(".instruction .text")?.textContent?.trim() === "Read the release notes and approve them."
      && opened.querySelector(".stage")?.textContent?.trim() === "CLAIM";
  }, MAIN)) && (await requestState(browser)).startsWith(`Request ${requestId} · DEFERRED · unclaimed · waiting `);
  return { human, exists };
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
  // UX journey: J-HUMAN-RED
  scenario<Record<string, never>>(
    "requests-verdict-switch",
    "switches between the GREEN and the RED form while editing",
    async () => prepareReview(),
    async (browser, { fixture }) => {
      const { human, exists } = opening(browser, signoff(fixture).id);
      await showDebug(browser);
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review" && await exists(), "the waiting review open at launch");
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().$("button*=Claim").click();
      await waitFor(browser, () => reviewing(browser), "claimed");
      await human().$("button=GREEN").click();
      const green = () => browser.$("#field-comment"), red = () => browser.$("#field-reason");
      await green().waitForDisplayed();
      assert.equal(await red().isExisting(), false, "no RED field in the GREEN form");
      await fill(browser, "#field-comment", "GREEN draft stays");
      await chooseVerdict(browser, "RED");
      await red().waitForDisplayed();
      assert.equal(await green().isExisting(), false, "no GREEN field in the RED form");
      await fill(browser, "#field-reason", "RED draft stays");
      await chooseVerdict(browser, "GREEN");
      await waitFor(browser, async () => await green().isDisplayed() && await green().getValue() === "GREEN draft stays", "GREEN restores its own draft");
      await chooseVerdict(browser, "RED");
      await waitFor(browser, async () => await red().isDisplayed() && await red().getValue() === "RED draft stays", "RED restores its own draft");
    },
  ),
  // UX journey: J-HUMAN-CLAIM
  scenario<Record<string, never>>(
    "requests-claim-elsewhere",
    "refuses a claim here while it is held, and offers it again once released",
    async () => prepareReview(),
    async (browser, { fixture }) => {
      const { text, human } = claim(browser);
      const id = signoff(fixture).id;
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review" && await human().isExisting(), "the waiting review open at launch");
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await showDebug(browser);
      await waitFor(browser, async () => (await requestState(browser)).includes("claimed by bob"), "bob's claim shown");
      // Taken: the review names who holds it beside its stage and offers no Claim button.
      await waitFor(browser, async () => (await human().$(".claimant").isExisting()) && (await human().$(".claimant").getText()) === "bob"
        && (await human().getText()).includes("Read-only until released.")
        && !(await human().$("button*=Claim").isExisting()), "bob's claim shown as read-only");
      await human().click();
      await press(browser, ["c"], () => browser.execute(() =>
        [...document.querySelectorAll('[aria-label="Human review"] [role="status"].error')].some((notice) => /bob/.test(notice.textContent ?? "")),
      ), "this opened review's claim refusal names bob");
      assert.equal(signoff(fixture).claim?.reviewer, "bob", "the claim stays bob's");
      assert.doesNotMatch(await text(), /claimed by you/);
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => signoff(fixture).claim === null, "released");
      await waitFor(browser, async () =>
        (await requestState(browser)).startsWith(`Request ${id} · DEFERRED · unclaimed · waiting `)
        && await human().$("button*=Claim").isEnabled(), "the loaded review shows the release and offers Claim again");
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed here after the release");
      await waitFor(browser, async () => (await requestState(browser)).includes("claimed by you (alice)"), "this review displays the new claim");
    },
  ),
  // UX journeys: J-HUMAN-TEXT J-HUMAN-GREEN
  scenario<Record<string, never>>(
    "requests-green-composition",
    "claims, composes Korean through the engine and guards page shortcuts before submitting GREEN",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { text, human } = submission(browser);
      await graphShown(browser, "a:release");
      // The Refresh observation sees a real F5 here, before it witnesses that composition
      // dispatched none.
      const unrefreshed = (await checkpoint(browser)).activity.refresh?.generation ?? 0;
      await press(browser, ["F5"], async () => ((await checkpoint(browser)).activity.refresh?.generation ?? 0) > unrefreshed, "a Refresh the observation sees");
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await human().waitForExist({ timeout: 15000, timeoutMsg: "the Human review" });
      await human().$("button*=Claim").waitForEnabled({ timeout: 15000 });
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice", "claimed");
      // The window shows the claim before a verdict can be chosen.
      await waitFor(browser, () => reviewing(browser), "the claim shown as the review stage");
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form of the pass schema");
      assert.equal(await browser.$("#field-approved").isExisting(), false, "the schema's approved constant is filled in, not shown");
      const comment = browser.$("#field-comment");
      await comment.click();
      type ImeEvent = { type: string; key: string; target: string; composing: boolean; keyCode: number; trusted: boolean };
      await browser.execute(() => {
        const log: ImeEvent[] = [];
        (window as unknown as { imeEvents: typeof log }).imeEvents = log;
        for (const type of ["compositionstart", "compositionupdate", "compositionend", "keydown"]) {
          window.addEventListener(type, (event) => {
            const key = event as KeyboardEvent;
            const target = event.target instanceof Element ? event.target.id : "";
            log.push({ type, key: key.key ?? "", target, composing: key.isComposing ?? false, keyCode: key.keyCode ?? 0, trusted: event.isTrusted });
          }, { capture: true });
        }
      });
      const events = () => browser.execute(() => (window as unknown as { imeEvents: ImeEvent[] }).imeEvents);
      const before = (await checkpoint(browser)).activity;
      await nativeInput(browser, { type: "set", text: "안" });
      await waitFor(browser, async () => (await events()).some((event) => event.type === "compositionstart" && event.trusted), "a trusted native composition started");
      await nativeInput(browser, { type: "set", text: "안녕" });
      await waitFor(browser, async () => await comment.getValue() === "안녕" && (await events()).some((event) =>
        event.type === "compositionupdate" && event.trusted), "the engine updated the native Korean composition");
      // E2E-INPUT: these fields are the app's own shortcut guard, not an OS input test.
      // Exercise each guard independently while the real native composition remains active.
      for (const [composing, keyCode] of [[true, 0], [false, 229]] as const) {
        // The review's own keys, then the window's: the palette, Open by ID, Settings, the Pane
        // and Refresh.
        for (const [key, primary] of [["g", false], ["r", false], ["q", false], ["u", false], ["c", false], ["Escape", false], ["g", true], ["r", true], ["s", true],
          ["k", true], ["o", true], [",", true], ["b", true], ["F5", false], ["?", false]] as const) {
          const observed = (await events()).length;
          await dispatchInput(browser, { kind: "composing-key", key, ...(primary ? { modifier: primaryModifier } : {}), composing, keyCode });
          const received = (await events()).slice(observed).filter((event) => event.type === "keydown" && !event.trusted);
          assert.deepEqual(received, [{ type: "keydown", trusted: false, target: "field-comment", composing, keyCode, key }],
            "the page's capture listener saw the guard fields on the composing input");
          // What this key would have done, had it not been guarded, before the next one.
          await commandsRendered(browser);
          const now = (await checkpoint(browser)).activity;
          assert.equal(await browser.$('[role="dialog"]').isExisting(), false, `${key}: no palette, Open by ID, Settings or Help`);
          assert.equal(await browser.$('input[aria-label="Search branches and reviews"]').isDisplayed(), true, `${key}: the Pane still shown`);
          assert.deepEqual(now.refresh, before.refresh, `${key}: no Refresh`);
          assert.ok(await browser.$("#field-comment").isExisting() && !(await browser.$("#field-reason").isExisting()), `${key}: the GREEN form still open`);
        }
      }
      assert.ok(!(await events()).some((event) => event.type === "compositionend"), "native composition stayed active through every guarded shortcut");
      await commandsRendered(browser);
      const after = (await checkpoint(browser)).activity;
      assert.ok(await comment.isExisting(), "the GREEN form stays open");
      assert.ok(!(await browser.$("#field-reason").isExisting()), "composition did not switch the verdict");
      assert.ok(!(await browser.$('[role="alertdialog"]').isExisting()), "composition did not leave or quit");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "composition did not release the claim");
      assert.equal(signoff(fixture).status, "DEFERRED", "composition submitted nothing");
      assert.equal(await browser.$('[role="dialog"]').isExisting(), false, "composition opened no palette, Open by ID, Settings or Help");
      assert.equal(await browser.$('input[aria-label="Search branches and reviews"]').isDisplayed(), true, "composition did not hide the Pane");
      assert.deepEqual(after.refresh, before.refresh, "composition dispatched no Refresh");
      await nativeInput(browser, { type: "commit", text: "안녕" });
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
