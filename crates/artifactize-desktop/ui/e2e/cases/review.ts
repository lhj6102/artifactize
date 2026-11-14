// A Human review in the Main area end to end: who claimed a waiting review in its row, opening
// one by its ID, leaving its form and then the review back to the Pane, quitting with an
// unsubmitted claim (stay, then release and quit), and the completed result kept open.
import assert from "node:assert/strict";
import { addRepository, artifactize, create, type Fixture, orderRuns, requests, signoff } from "../fixture.ts";
import { claimable, requestState, showDebug, reviewing } from "../human-review.ts";
import { fill, press, typeText } from "../keys.ts";
import { sessionEndedByApp } from "../observation.ts";
import { commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier, requestWindowClose } from "../platform.ts";
import { branch, entriesFocused, expand, mainTitle, waiting } from "../pane.ts";

/** How many Human reviews this case's state holds waiting, read through the public CLI. */
function waitingReviews(fixture: Fixture): number {
  return requests(fixture).filter((request) => request.status === "DEFERRED").length;
}

async function waitFor(browser: WebdriverIO.Browser, check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  await browser.waitUntil(async () => !!(await check()), { timeout: ms, timeoutMsg: `timed out: ${what}` });
}

/** The app started with `review`: the waiting sign-off opens in the Main area; claim it. */
async function claimReview(browser: WebdriverIO.Browser, fixture: Fixture) {
  await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "the waiting review open at launch");
  await browser.waitUntil(() => claimable(browser), { timeout: 15000, timeoutMsg: "the claim stage" });
  await browser.$('[aria-label="Human review"]').click();
  await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "this case's claim");
}

/** Open `id` with Ctrl+O and wait for its review. */
async function openById(browser: WebdriverIO.Browser, id: string) {
  await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
  await fill(browser, '[aria-label="Request ID"]', id);
  await browser.$('[role="dialog"] button[type="submit"]').click();
  await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "a Review in the Main area");
  await showDebug(browser);
  await waitFor(browser, async () => (await requestState(browser)).startsWith(`Request ${id} · `), `${id}'s review`);
}

export const scenarios: Scenario[] = [
  // UX journeys: J-WAITING J-LIVE; UX edge: E-MON-LIVE
  scenario<null>(
    "review-waiting-claim",
    "shows in a waiting review's row who claimed it, as another process claims and releases it",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const id = signoff(fixture).id;
      const row = async () => (await waiting(browser)).rows.find((entry) => entry.id === id);
      await waitFor(browser, async () => (await row()) !== undefined, "the waiting row");
      const shown = (await row())!;
      assert.deepEqual([shown.eval, shown.where], ["release/signoff", "repo  master"]);
      assert.doesNotMatch(shown.meta, / · /, "nobody claimed it: only how long it waits");
      assert.equal(signoff(fixture).claim, null, "listing claims nothing");
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => / · bob$/.test((await row())?.meta ?? ""), "bob's claim in the row");
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await waitFor(browser, async () => { const meta = (await row())?.meta; return meta !== undefined && !/ · /.test(meta); }, "bob's release in the row");
      assert.equal(await mainTitle(browser), "Nothing chosen", "nothing opened by itself");
    },
  ),

  // UX journeys: J-OPEN-ID J-WAITING; UX edges: E-REVIEW-OPEN-ID E-WAITING-CHANGE E-HUMAN-CLAIM
  scenario<null>(
    "review-open-id",
    "opens a waiting review by its ID, actionable, and claims it, while another repository's review arrives in the stack",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const original = signoff(fixture).id;
      await waitFor(browser, async () => (await waiting(browser)).bar === "Waiting (1)", "the one waiting sign-off counted");
      const before = new Set(requests(fixture).map((request) => request.id));
      await addRepository(fixture, "open-by-id");
      const added = requests(fixture).find((request) => request.evalId === "release/signoff" && !before.has(request.id));
      assert.ok(added, "the other repository created its own sign-off");
      // Explicitly the newer, whatever times the two verifies were saved at.
      orderRuns(fixture, [requests(fixture).find((request) => request.id === original)!.runId, added.runId]);
      await waitFor(browser, async () => (await waiting(browser)).bar === "Waiting (2)", "the other repository's sign-off counted without a reload");
      assert.equal(waitingReviews(fixture), 2);
      await commandsRendered(browser);
      // The older one by its ID, not the newest at the top of the stack.
      assert.equal((await waiting(browser)).rows[0].id, added.id);
      await openById(browser, original);
      // Debug, unfolded to read the request's state above, folds and unfolds by t.
      const technical = () => browser.$('[aria-label="Human review"]').$("button.head=Debug").getAttribute("aria-expanded");
      await browser.$('[aria-label="Human review"]').click();
      await press(browser, ["t"], async () => (await technical()) === "false", "Debug folded by t");
      await press(browser, ["t"], async () => (await technical()) === "true", "the opened review's identity");
      const openedId = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Human review"] dt')]
        .find((field) => field.textContent === "Request")?.nextElementSibling?.textContent ?? "");
      await waitFor(browser, async () => (await openedId()) === original, "the review is exactly the entered request");
      await browser.waitUntil(() => claimable(browser), { timeout: 15000, timeoutMsg: "the claim stage" });
      await press(browser, ["c"], async () =>
        requests(fixture).find((request) => request.id === original)?.claim?.reviewer === "alice" &&
        await reviewing(browser, '[aria-label="Human review"][tabindex="0"]'), "the entered request's saved claim and rendered review");
      assert.equal(requests(fixture).find((request) => request.id === added.id)?.claim, null, "the other review was not claimed");
    },
  ),

  // UX journey: J-HUMAN-ESC; UX edges: E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL E-REVIEW-OPEN
  scenario<null>(
    "review-escape-form-detail",
    "leaves a field's form with Esc, then the review back to the Pane with the next Esc, and opens it again with Enter, keyboard only",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      await claimReview(browser, fixture);
      await browser.keys(["g"]);
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.id === "field-comment"), { timeout: 15000, timeoutMsg: "focus in the comment field" });
      await fill(browser, "#field-comment", "kept draft");
      await browser.keys(["Escape"]);
      await browser.$("#field-comment").waitForExist({ reverse: true, timeout: 10000 });
      assert.ok(await browser.$('[aria-label="Human review"]').isExisting(), "the review stays");
      await press(browser, ["Escape"], () => entriesFocused(browser), "back in the Pane");
      assert.equal((await waiting(browser)).rows.find((row) => row.selected)?.id, signoff(fixture).id, "its row stays selected");
      // Enter opens the same review again, its claim and draft kept.
      // Enter moves the keyboard from the Pane into the review it opens.
      const inReview = () => browser.execute(() => !!document.activeElement?.closest('[aria-label="Human review"]'));
      await press(browser, ["Enter"], async () => !(await entriesFocused(browser)) && await inReview() && (await mainTitle(browser)) === "Review" && await reviewing(browser),
        "the same review again, with the keyboard");
      assert.equal(signoff(fixture).claim?.reviewer, "alice");
      await press(browser, ["g"], async () => (await browser.$("#field-comment").isExisting()) && (await browser.$("#field-comment").getValue()) === "kept draft", "the kept draft");
    },
  ),

  // UX journey: J-QUIT; UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
  scenario<null>(
    "review-quit-claim",
    "asks before quitting with the claim: Esc stays, u releases and quits",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      await claimReview(browser, fixture);
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
      await requestWindowClose(browser);
      await requestWindowClose(browser);
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
      assert.equal(signoff(fixture).status, "DEFERRED");
    },
  ),

  // UX journeys: J-HUMAN-GREEN J-WAITING; UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN E-HUMAN-SUBMIT E-WAITING-CHANGE
  scenario<null>(
    "review-green-completed",
    "keeps the completed result open until the reviewer leaves it, the Waiting stack and the branch's chip following",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      assert.equal(waitingReviews(fixture), 1, "this case starts with one waiting sign-off");
      await claimReview(browser, fixture);
      assert.equal((await waiting(browser)).bar, "Waiting (1)");
      await browser.keys(["g"]);
      const comment = await browser.$("#field-comment");
      await comment.waitForExist({ timeout: 10000 });
      await comment.click();
      await typeText(browser, "looks good");
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "looks good" });
      assert.equal(waitingReviews(fixture), 0, "the submission left nothing waiting");
      // Nothing waits any more: the bar reads (0), and the branch's Run ends GREEN.
      await waitFor(browser, async () => (await waiting(browser)).bar === "Waiting (0)", "the stack empty after the submission");
      const run = signoff(fixture).runId;
      await expand(browser, "repo  master");
      await waitFor(browser, async () => /^GREEN /.test((await branch(browser, "repo  master"))?.chips.find((chip) => chip.id === run)?.text ?? ""),
        "the app has rendered this Run's terminal GREEN update on its chip");
      await commandsRendered(browser);
      const human = browser.$('[aria-label="Human review"]');
      assert.ok(await human.isExisting(), "the completed review stays open after the terminal update");
      assert.match(await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? ""), /completed result/i);
      assert.equal(await mainTitle(browser), "Review · read-only", "completed, the review is read-only");
    },
  ),
  // UX journey: J-HUMAN-LAYOUT; UX edges: E-HUMAN-CLAIM E-HUMAN-GREEN
  scenario<null>(
    "review-layout",
    "lays a review in the Main area out as two panels where it is at least twice the Detail's width, the Tool log the view at the left and the rest a column at the right, and in one column where it is narrower",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      await browser.setWindowSize(1920, 1080);
      await claimReview(browser, fixture);
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      /** The review's arrangement: its panels' boxes, its Result's and its row of actions' boxes. */
      const arranged = () => browser.execute(() => {
        const review = document.querySelector<HTMLElement>('[aria-label="Human review"]')!;
        const box = (element: Element | null | undefined) => {
          const rect = element?.getBoundingClientRect();
          return rect ? { left: Math.round(rect.left), right: Math.round(rect.right), top: Math.round(rect.top), bottom: Math.round(rect.bottom) } : null;
        };
        const result = review.querySelector('section[aria-label="Result"]');
        const sections = [...review.querySelectorAll(".section")].map((section) => section.getAttribute("aria-label"));
        return {
          area: box(review),
          split: review.classList.contains("split"),
          viewer: box(review.querySelector(".viewer")),
          log: box(review.querySelector(".viewer [role='log']")),
          side: box(review.querySelector(".side")),
          result: box(result),
          // The row of actions is the Result's last row.
          actionsLast: !!result && result.querySelector(".body > .result > :last-child")?.classList.contains("actions") === true,
          sections,
        };
      });
      // Wide: the Tool log fills the left, from below the header row to the bottom; the column at the right.
      await browser.waitUntil(async () => (await arranged()).split, { timeout: 15000, timeoutMsg: "two panels in a wide Main area" });
      const wide = await arranged();
      assert.ok(wide.viewer && wide.side && wide.log && wide.area, JSON.stringify(wide));
      assert.ok(wide.viewer.right <= wide.side.left + 1 && wide.side.right >= wide.area.right - 1, `the Tool log at the left, the column at the right: ${JSON.stringify(wide)}`);
      assert.ok(wide.log.bottom >= wide.area.bottom - 2 && wide.viewer.top <= wide.area.top + 2, `the Tool log fills its panel's height: ${JSON.stringify(wide)}`);
      assert.deepEqual(wide.sections, ["Tools", "Result", "Debug"], "the column holds the Tools, the Result and Debug");
      assert.ok(wide.actionsLast, "the row of actions ends the Result");
      // While a tool runs, its time and Cancel stand between Release claim and Submit; Cancel
      // stops it, the review and its draft staying.
      await fill(browser, "#field-comment", "draft kept through the tool");
      await browser.$('[aria-label="Human review"]').$("button=slow").click();
      const row = () => browser.execute(() => {
        const actions = document.querySelector('[aria-label="Human review"] .actions');
        const at = (selector: string) => actions?.querySelector(selector)?.getBoundingClientRect().left ?? null;
        return {
          release: at('button[aria-label="Release claim (u)"]'),
          seconds: actions?.querySelector(".busy .seconds")?.textContent?.trim() ?? "",
          cancel: at('button[aria-label="Cancel (Esc)"]'),
          submit: at("button.primary"),
        };
      });
      let running = await row();
      await browser.waitUntil(async () => /^\d+s$/.test((running = await row()).seconds), { timeout: 15000, timeoutMsg: "the running tool's time in the row" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(running)}`); });
      assert.ok(running.release !== null && running.cancel !== null && running.submit !== null && running.release < running.cancel && running.cancel < running.submit,
        `Release claim, then the running tool's time and Cancel, then Submit: ${JSON.stringify(running)}`);
      await browser.$('[aria-label="Human review"] button[aria-label="Cancel (Esc)"]').click();
      await browser.waitUntil(async () => (await row()).seconds === "", { timeout: 15000, timeoutMsg: "Cancel stopped the tool" });
      assert.equal(await browser.$("#field-comment").getValue(), "draft kept through the tool", "the draft stays");
      // Narrow: one column, top to bottom, the Tool log a box between the Tools and the Result.
      await browser.setWindowSize(800, 760);
      await browser.waitUntil(async () => !(await arranged()).split, { timeout: 15000, timeoutMsg: "one column in a narrow Main area" });
      const narrow = await arranged();
      assert.deepEqual(narrow.sections, ["Tools", "Tool log", "Result", "Debug"], "one column, the Tool log between the Tools and the Result");
      assert.ok(narrow.actionsLast, "the row of actions still ends the Result");
      assert.equal(await browser.$("#field-comment").isExisting(), true, "the form kept through the change");
    },
  ),
];
