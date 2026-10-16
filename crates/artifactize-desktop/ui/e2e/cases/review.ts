// The standalone review end to end: the waiting list, opening a request by ID, and quitting
// with an unsubmitted claim (stay, then release and quit).
import assert from "node:assert/strict";
import { addRepository, artifactize, create, type Fixture, requests, signoff } from "../fixture.ts";
import { press, typeText } from "../keys.ts";
import { sessionEndedByApp } from "../observation.ts";
import { commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";

async function claimReview(browser: WebdriverIO.Browser, fixture: Fixture) {
  const text = () => browser.execute(() => document.body.innerText);
  await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
  await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
  const input = await browser.$('[aria-label="Request ID"]');
  await input.setValue(signoff(fixture).id);
  await browser.$('[role="dialog"] button[type="submit"]').click();
  await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 15000 });
  await browser.waitUntil(async () => (await text()).includes("Unclaimed. c claims it for alice"), { timeout: 15000, timeoutMsg: "the claim stage" });
  await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && (await text()).includes("REVIEW (yours)"), "this case's claim");
}

export const scenarios: Scenario[] = [
  // UX edges: E-REVIEW-PREVIEW
  scenario<null>(
    "review-preview",
    "lists the waiting sign-off with a read-only preview",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      const list = await browser.$('[aria-label="Requests"]');
      await list.waitForExist({ timeout: 15000 });
      // Observe the rendered grid text, including cells omitted by an element text query.
      const shown = () => browser.execute((element) => (element as HTMLElement).innerText, list);
      await browser.waitUntil(async () => (await shown()).includes("release/signoff"), { timeout: 15000, timeoutMsg: "the waiting row" });
      await browser.waitUntil(async () => (await text()).includes("Enter or click: open review"), { timeout: 15000, timeoutMsg: "the preview" });
      assert.equal(signoff(fixture).claim, null, "previewing claims nothing");

      // Observe the selected request's list cell and Preview independently: text from one
      // cannot stand in for the other when a subscription update has not reached both.
      const id = signoff(fixture).id;
      const claimant = () => browser.execute((requestId) => {
        const row = [...document.querySelectorAll('[aria-label="Requests"] [role="option"]')]
          .find((entry) => entry.querySelector(".mono")?.textContent?.trim() === requestId);
        const preview = document.querySelector('section.panel[aria-label="Preview"] .preview');
        return {
          selected: row?.getAttribute("aria-selected") === "true",
          claim: row?.children[3]?.textContent?.trim(),
          preview: preview?.querySelector(".meta")?.textContent?.trim() ?? "",
        };
      }, id);
      await browser.waitUntil(async () => {
        const shown = await claimant();
        return shown.selected && shown.claim === "-" && shown.preview.startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `);
      }, { timeout: 15000, timeoutMsg: "the identified request starts unclaimed in both its row and Preview" });
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => {
        const shown = await claimant();
        return shown.selected && shown.claim === "bob" && shown.preview.startsWith(`Request ${id} · WAITING_HUMAN · claimed by bob · read-only · waiting `);
      }, { timeout: 15000, timeoutMsg: "the same selected row and Preview both display bob's claim" });
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => {
        const shown = await claimant();
        return shown.selected && shown.claim === "-" && shown.preview.startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `);
      }, { timeout: 15000, timeoutMsg: "bob's release is rendered in both the same row and Preview" });
    },
  ),

  // UX edges: E-REVIEW-OPEN-ID E-HUMAN-CLAIM
  scenario<null>(
    "review-open-id",
    "opens the entered request ID rather than the selected request and claims it",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      await browser.$('[aria-label="Requests"] [aria-selected="true"]').waitForExist({ timeout: 15000 });
      const original = signoff(fixture).id;
      const before = new Set(requests(fixture).map((request) => request.id));
      await addRepository(fixture, "open-by-id");
      const added = requests(fixture).find((request) => request.evalId === "release/signoff" && !before.has(request.id));
      assert.ok(added, "the other repository created its own sign-off");
      const otherId = added.id;
      assert.notEqual(otherId, original, "different repositories own independent reviews, not followers");
      await browser.waitUntil(() => browser.execute((ids) => {
        const listed = [...document.querySelectorAll('[aria-label="Requests"] .mono')].map((cell) => cell.textContent);
        return ids.every((id) => listed.includes(id));
      }, [original, otherId]), { timeout: 15000, timeoutMsg: "both independent requests are in this case's list" });
      await commandsRendered(browser);
      // A live insertion may select either row. Read the actual selected identity rather
      // than assuming the older request stayed selected. Read the rendered grid cells.
      const selected = await browser.execute(() => document.querySelector('[aria-label="Requests"] [aria-selected="true"] .mono')?.textContent ?? "");
      assert.ok([original, otherId].includes(selected));
      const id = selected === original ? otherId : original;
      assert.notEqual(id, selected, "the typed ID differs from the exact currently selected request");
      await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
      await browser.$('[aria-label="Request ID"]').setValue(id);
      await browser.$('[role="dialog"] button[type="submit"]').click();
      await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 15000 });
      await browser.waitUntil(async () => (await browser.$('[aria-label="Human review"] .meta').getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `), {
        timeout: 15000, timeoutMsg: "the entered Human request's exact identity is visible by default, before Technical",
      });
      const technical = () => browser.$('[aria-label="Human review"]').$("button*=Technical").getAttribute("aria-expanded");
      await press(browser, ["t"], async () => (await technical()) === "true", "the opened review's identity");
      const openedId = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Human review"] dt')]
        .find((field) => field.textContent === "Request")?.nextElementSibling?.textContent ?? "");
      await browser.waitUntil(async () => (await openedId()) === id, { timeout: 15000, timeoutMsg: "Detail identifies exactly the entered request" });
      assert.notEqual(await openedId(), selected);
      await browser.waitUntil(async () => (await text()).includes("Unclaimed. c claims it for alice"), { timeout: 15000, timeoutMsg: "the claim stage" });
      await press(browser, ["c"], async () =>
        requests(fixture).find((request) => request.id === id)?.claim?.reviewer === "alice" &&
        (await openedId()) === id &&
        await browser.execute(() => document.querySelector('[aria-label="Human review"][tabindex="0"]')
          ?.textContent?.includes("REVIEW (yours)") ?? false), "the entered request's saved claim and rendered review");
      // request list reaches every repository in this state: observe by exact identities,
      // not by calling a first-signoff helper with another --repo path.
      const previous = requests(fixture).find((request) => request.id === selected);
      assert.ok(previous, "the previously selected request is still saved");
      assert.equal(previous.claim, null, "the previously selected request was not claimed");
    },
  ),

  // UX edges: E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL
  scenario<null>(
    "review-escape-form-detail",
    "leaves a field's form with Esc, then the Detail with the next Esc, keyboard only",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      await claimReview(browser, fixture);
      await browser.waitUntil(async () => (await text()).includes("REVIEW (yours)"), { timeout: 15000, timeoutMsg: "the review stage" });
      await browser.keys(["g"]);
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.id === "field-comment"), { timeout: 15000, timeoutMsg: "focus in the comment field" });
      await browser.keys(["Escape"]);
      await browser.$("#field-comment").waitForExist({ reverse: true, timeout: 10000 });
      assert.ok(await browser.$('[aria-label="Human review"]').isExisting(), "the Detail stays");
      await browser.keys(["Escape"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
      // Back on the list: Enter opens the same request again, its claim kept.
      await browser.keys(["Enter"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 10000 });
    },
  ),

  // UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
  scenario<null>(
    "review-quit-claim",
    "asks before quitting with the claim: Esc stays, u releases and quits",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      await claimReview(browser, fixture);
      await browser.waitUntil(async () => (await text()).includes("REVIEW (yours)"), { timeout: 15000, timeoutMsg: "the review stage" });
      const dialog = await browser.$('[role="alertdialog"]');
      /** The prompt is open and holds focus, so its keys reach it. */
      const prompted = async () => {
        await browser.keys(["q"]);
        await dialog.waitForExist({ timeout: 10000 });
        await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')), { timeout: 15000, timeoutMsg: "focus in the prompt" });
      };
      await prompted();
      const shown = await browser.execute((element) => (element as HTMLElement).innerText, dialog);
      assert.match(shown, new RegExp(signoff(fixture).id));
      await browser.keys(["Escape"]);
      await dialog.waitForExist({ reverse: true, timeout: 10000 });
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "staying keeps the claim");
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[aria-label="Human review"]')), { timeout: 15000, timeoutMsg: "focus back in the review" });

      // The window's own close requests, twice in a row: both wait for the prompt's answer.
      await browser.closeWindow();
      await browser.closeWindow();
      await dialog.waitForExist({ timeout: 10000 });
      assert.equal(await browser.execute(() => document.title), "Artifactize", "the window stays");
      assert.equal(signoff(fixture).claim?.reviewer, "alice");
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')), { timeout: 15000, timeoutMsg: "focus in the prompt" });
      await browser.keys(["Escape"]);
      await dialog.waitForExist({ reverse: true, timeout: 10000 });

      await prompted();
      // A disappearing window may end this keystroke's response, but a generic driver
      // failure is not closure. The native helper only accepts a terminal window outcome.
      await sessionEndedByApp(browser, () => browser.keys(["u"]));
      await browser.waitUntil(() => signoff(fixture).claim === null, { timeout: 15000, timeoutMsg: "released on quit" });
      assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    },
  ),

  // UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN E-HUMAN-SUBMIT
  scenario<null>(
    "review-green-completed",
    "keeps the completed result open until the reviewer leaves it",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
      await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
      const input = await browser.$('[aria-label="Request ID"]');
      await input.setValue(signoff(fixture).id);
      await browser.$('[role="dialog"] button[type="submit"]').click();
      const human = await browser.$('[aria-label="Human review"]');
      await human.waitForExist({ timeout: 15000 });
      await browser.waitUntil(async () => (await text()).includes("Unclaimed. c claims it for alice"), { timeout: 15000, timeoutMsg: "the claim stage" });
      await browser.keys(["c"]);
      await browser.waitUntil(async () => signoff(fixture).claim?.reviewer === "alice", { timeout: 15000, timeoutMsg: "claimed" });
      await browser.waitUntil(async () => (await text()).includes("REVIEW (yours)"), { timeout: 15000, timeoutMsg: "the review stage" });
      await browser.keys(["g"]);
      const comment = await browser.$("#field-comment");
      await comment.waitForExist({ timeout: 10000 });
      await comment.click();
      await typeText(browser, "looks good");
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "looks good" });
      const run = signoff(fixture).runId;
      // Monitor stays mounted while Review is shown. Its exact Run row acknowledges the
      // app's terminal subscription update without leaving this completed Review Detail.
      await browser.waitUntil(() => browser.execute((id) => {
        const row = [...document.querySelectorAll('.monitor [aria-label="Runs"] [role="option"]')]
          .find((entry) => entry.querySelector(".id")?.textContent?.split(/\s/)[0] === id);
        return row?.querySelector(".status")?.textContent?.trim() === "GREEN";
      }, run), { timeout: 15000, timeoutMsg: "the app has rendered this Run's terminal GREEN update" });
      await commandsRendered(browser);
      assert.ok(await human.isExisting(), "the completed Detail stays open after the terminal subscription update");
      const completed = await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
      assert.match(completed, /completed result/i);
    },
  ),
];
