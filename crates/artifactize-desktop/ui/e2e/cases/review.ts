// A Human review in the Main area end to end: who claimed a waiting review in its row, opening
// one by its ID, leaving its form and then the review back to the Pane, quitting with an
// unsubmitted claim (stay, then release and quit), and the completed result kept open.
import assert from "node:assert/strict";
import { addRepository, addWaiting, artifactize, create, type Fixture, orderRuns, requests, savedRuns, signoff, slowTool } from "../fixture.ts";
import { claimable, requestState, showDebug, reviewing } from "../human-review.ts";
import { afterRender, fill, press, typeText } from "../keys.ts";
import { sessionEndedByApp } from "../observation.ts";
import { clickAt, visibleCenter } from "../pointer.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { nativePath, readText, removeTree, requestWindowClose, writeText } from "../platform.ts";
import { branch, entriesFocused, expand, MAIN, mainTitle, openWaiting, showRun, waiting } from "../pane.ts";
import { detail, openArtifact, openTab } from "../graph-view.ts";

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
  // The visible way: the palette's Open a request by ID, clicked.
  await browser.$("header.bar").$("button*=Commands").click();
  await choosePaletteCommand(browser, "Open a request by ID");
  await browser.$('.palette [role="option"][aria-selected="true"]').click();
  await waitFor(browser, () => browser.$('[aria-label="Request ID"]').isExisting(), "the Request ID dialog");
  await fill(browser, '[aria-label="Request ID"]', id);
  await browser.$('[role="dialog"] button[type="submit"]').click();
  await waitFor(browser, async () => (await mainTitle(browser)) === "Review", "a Review in the Main area");
  await showDebug(browser);
  await waitFor(browser, async () => (await requestState(browser)).startsWith(`Request ${id} · `), `${id}'s review`);
}

export const scenarios: Scenario[] = [
  // UX journeys: J-WAITING J-LIVE
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

  // UX journeys: J-OPEN-ID J-WAITING
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
      await browser.$('[aria-label="Human review"]').$("button=Claim to review").click();
      await waitFor(browser, async () =>
        requests(fixture).find((request) => request.id === original)?.claim?.reviewer === "alice" &&
        await reviewing(browser, '[aria-label="Human review"][tabindex="0"]'), "the entered request's saved claim and rendered review");
      assert.equal(requests(fixture).find((request) => request.id === added.id)?.claim, null, "the other review was not claimed");
    },
  ),

  // UX journey: J-HUMAN-ESC
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

  // UX journey: J-QUIT
  scenario<{ second: string }>(
    "review-quit-claim",
    "asks before quitting with two claims; Stay keeps both, Release and quit releases both and closes the window",
    async () => {
      const fixture = await create();
      const [second] = await addWaiting(fixture, "second-review", 1);
      assert.ok(second);
      return { fixture, args: ["review", signoff(fixture).id], data: { second } };
    },
    async (browser, { fixture, data: { second } }) => {
      await claimReview(browser, fixture);
      const dialog = () => browser.$('[role="alertdialog"]');
      const field = browser.$("#field-comment");
      await browser.$('[aria-label="Human review"]').$("button=GREEN").click();
      await field.waitForDisplayed();
      await field.click();
      await waitFor(browser, () => field.isFocused(), "the comment field takes the keyboard");
      await typeText(browser, "q");
      await afterRender(browser);
      assert.equal(await dialog().isExisting(), false, "q in a field does not open Leave");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "typing leaves the window and its claim alone");
      await press(browser, ["Escape"], async () => !(await field.isExisting()) && await reviewing(browser), "left the form before trying q outside it");
      await openWaiting(browser, second);
      await waitFor(browser, () => claimable(browser), "the second review asks for a claim");
      await browser.$('[aria-label="Human review"]').$("button=Claim to review").click();
      const ids = [signoff(fixture).id, second];
      const held = () => requests(fixture).filter((request) => ids.includes(request.id));
      const bothClaimed = () => {
        const found = held();
        return found.length === 2 && found.every((request) => request.claim?.reviewer === "alice");
      };
      await waitFor(browser, async () => bothClaimed() && await reviewing(browser), "the same window holds both claims");
      await requestWindowClose(browser);
      await waitFor(browser, async () => {
        if (!(await dialog().isDisplayed())) return false;
        const text = await dialog().getText();
        return ids.every((id) => text.includes(id));
      }, "Leave visibly names both claims");
      await dialog().$("button*=Stay").click();
      await dialog().waitForExist({ reverse: true, timeout: 10000 });
      assert.ok(bothClaimed(), "staying keeps both claims");
      await waitFor(browser, () => reviewing(browser), "the review stays open");

      // The window's own close requests, twice in a row: both wait for the prompt's answer.
      await requestWindowClose(browser);
      await requestWindowClose(browser);
      await dialog().waitForExist({ timeout: 10000 });
      assert.equal(await browser.execute(() => document.title), "Artifactize", "the window stays");
      assert.ok(bothClaimed(), "neither close request released a claim before the answer");
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')), { timeout: 15000, timeoutMsg: "focus in the prompt" });
      // A disappearing window may end the click's response. The native helper only accepts
      // a terminal window outcome, and both requests must still exist without a claim.
      const leave = await visibleCenter(browser, await dialog().$("button*=Release and quit").getElement());
      await sessionEndedByApp(browser, () => clickAt(browser, leave));
      await waitFor(browser, () => {
        const found = held();
        return found.length === 2 && found.every((request) => request.claim === null && request.status === "DEFERRED");
      }, "both reviews are unclaimed and waiting after the window quit");
    },
  ),

  // UX journeys: J-HUMAN-GREEN J-WAITING
  scenario<null>(
    "review-green-completed",
    "keeps the completed result open until the reviewer leaves it, the Waiting stack and the branch's chip following",
    async () => ({ fixture: await create(), args: ["review"], data: null }),
    async (browser, { fixture }) => {
      assert.equal(waitingReviews(fixture), 1, "this case starts with one waiting sign-off");
      const human = browser.$('[aria-label="Human review"]');
      await human.$("button=Claim to review").click();
      await waitFor(browser, async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "the Claim to review button took the claim");
      assert.equal((await waiting(browser)).bar, "Waiting (1)");
      await human.$("button=GREEN").click();
      const comment = await browser.$("#field-comment");
      await comment.waitForExist({ timeout: 10000 });
      await comment.click();
      await typeText(browser, "looks good");
      await human.$("button=Submit GREEN").click();
      await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "looks good" });
      assert.equal(waitingReviews(fixture), 0, "the submission left nothing waiting");
      // Nothing waits any more: the bar reads (0), and the branch's Run ends GREEN.
      await waitFor(browser, async () => (await waiting(browser)).bar === "Waiting (0)", "the stack empty after the submission");
      const run = signoff(fixture).runId;
      await expand(browser, "repo  master");
      await waitFor(browser, async () => /^GREEN /.test((await branch(browser, "repo  master"))?.chips.find((chip) => chip.id === run)?.text ?? ""),
        "the app has rendered this Run's terminal GREEN update on its chip");
      const completed = human.$('section[aria-label="Result"] pre');
      await waitFor(browser, async () => (await mainTitle(browser)) === "Review · read-only"
        && await completed.isDisplayed() && (await completed.getText()).includes("looks good"), "the submitted review stayed open as a completed result");
      await showDebug(browser);
      await waitFor(browser, async () => (await requestState(browser)).startsWith(`Request ${signoff(fixture).id} · GREEN`), "the same completed request is shown");
      assert.match(await human.getText(), /completed result/i);
      const result = await completed.getText();
      assert.match(result, /^GREEN\n/);
      assert.deepEqual(JSON.parse(result.slice(result.indexOf("\n") + 1)), { verdict: "GREEN", approved: true, comment: "looks good" });
      assert.equal(await mainTitle(browser), "Review · read-only", "completed, the review is read-only");
    },
  ),
  // UX journeys: J-HUMAN-LAYOUT J-HUMAN-CLAIM
  scenario<null>(
    "review-layout",
    "lays a review in the Main area out as two panels where it is at least twice the Detail's width, the Tool log the view at the left and the rest a column at the right, and in one column where it is narrower",
    async () => ({ fixture: await create({ title: "A person approves the release", instruction: "Read the release notes and approve them.", gatedFingerprint: true }),
      args: ["review"], data: null }),
    async (browser, { fixture }) => {
      // The Detail's width as a Run's graph opens it, and the Main area's, at a window size.
      const run = savedRuns(fixture)[0].id;
      const measured = async (width: number, height: number) => {
        await browser.setWindowSize(width, height);
        await showRun(browser, "repo  master", run);
        await openArtifact(browser, "a:release", "Artifact release");
        const sizes = await browser.execute(() => ({
          detail: Math.round(document.querySelector('.stage aside[aria-label="Detail"]')!.getBoundingClientRect().width),
          main: Math.round(document.querySelector(".stage")!.getBoundingClientRect().width),
        }));
        await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
        return sizes;
      };
      const narrowSizes = await measured(800, 760);
      const wideSizes = await measured(1920, 1080);
      assert.ok(wideSizes.main >= 2 * wideSizes.detail && narrowSizes.main < 2 * narrowSizes.detail,
        `the window sizes fall on both sides of twice the Detail's width: ${JSON.stringify({ wideSizes, narrowSizes })}`);
      await openWaiting(browser, signoff(fixture).id);
      await claimReview(browser, fixture);
      const title = "release/signoff · A person approves the release";
      const checkHeadingAndDebug = async (shell: string, heading: string, viaKey: boolean) => {
        const review = browser.$(`${shell} [aria-label="Human review"]`);
        const debug = review.$("button.head=Debug");
        const id = signoff(fixture).id;
        const countTitle = () => browser.execute((selector, text) => {
          const content = document.querySelector<HTMLElement>(selector)?.innerText ?? "";
          return content.split(text).length - 1;
        }, shell, "A person approves the release");
        await waitFor(browser, async () => await browser.$(heading).getText() === title && await countTitle() === 1,
          "the eval title is rendered only once, in the review heading");
        const state = () => requestState(browser, `${shell} [aria-label="Human review"]`);
        assert.equal(await debug.getAttribute("aria-expanded"), "false", "Debug starts folded");
        assert.equal(await state(), "");
        assert.ok(!(await review.getText()).includes(id), "the request facts are not shown outside Debug");
        if (viaKey) {
          await review.$('[aria-label="Instruction"]').click();
          await press(browser, ["t"], async () => (await state()).includes(id), "t shows the request facts");
        } else await debug.click();
        await waitFor(browser, async () => (await state()).includes(`Request ${id} · DEFERRED · claimed by you (alice) · waiting `)
          && / · waiting \d/.test(await state()), "Debug shows the request ID, deferred mode, claimant, and waiting time");
        if (viaKey) await press(browser, ["t"], async () => (await state()) === "", "t hides the request facts again");
        else await debug.click();
        await waitFor(browser, async () => (await debug.getAttribute("aria-expanded")) === "false" && (await state()) === "", "Debug folds away its facts");
      };
      await checkHeadingAndDebug(MAIN, `${MAIN} > header .title`, true);
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      /** The review's arrangement: its panels' boxes, its Result's and its row of actions' boxes. */
      const arranged = () => browser.execute(() => {
        const review = document.querySelector<HTMLElement>('[aria-label="Human review"]')!;
        const box = (element: Element | null | undefined) => {
          const rect = element?.getBoundingClientRect();
          return rect ? { left: Math.round(rect.left), right: Math.round(rect.right), top: Math.round(rect.top), bottom: Math.round(rect.bottom) } : null;
        };
        const result = review.querySelector('section[aria-label="Result"]');
        const instruction = review.querySelector('[aria-label="Instruction"]');
        const tools = review.querySelector('section[aria-label="Tools"]');
        const toolLog = review.querySelector('section[aria-label="Tool log"]');
        const sections = [...review.querySelectorAll(".section")].map((section) => section.getAttribute("aria-label"));
        return {
          area: box(review),
          split: review.classList.contains("split"),
          viewer: box(review.querySelector(".viewer")),
          log: box(review.querySelector(".viewer [role='log']")),
          side: box(review.querySelector(".side")),
          result: box(result),
          instruction: box(instruction),
          tools: box(tools),
          toolLog: box(toolLog),
          measure: Number.parseFloat(getComputedStyle(review).getPropertyValue("--measure")),
          // The row of actions is the Result's last row.
          actionsLast: !!result && result.querySelector(".body > .result > :last-child")?.classList.contains("actions") === true,
          sections,
        };
      });
      const oneColumn = (layout: Awaited<ReturnType<typeof arranged>>) => {
        const { area, instruction, tools, toolLog, result, measure } = layout;
        if (!area || !instruction || !tools || !toolLog || !result || !Number.isFinite(measure)) return false;
        const rows = [instruction, tools, toolLog, result];
        return rows.every((row, index) => row.right > row.left && row.bottom > row.top &&
          Math.abs(row.left - instruction.left) <= 1 && Math.abs(row.right - instruction.right) <= 1 &&
          row.left >= area.left && row.right <= area.right && row.right - row.left <= measure + 1 &&
          (index === 0 || row.top >= rows[index - 1].bottom - 1));
      };
      // Wide: the Tool log fills the left, from below the header row to the bottom; the column at the right.
      await browser.waitUntil(async () => (await arranged()).split, { timeout: 15000, timeoutMsg: "two panels in a wide Main area" });
      const wide = await arranged();
      assert.ok(wide.viewer && wide.side && wide.log && wide.area, JSON.stringify(wide));
      assert.ok(wide.viewer.right <= wide.side.left + 1 && wide.side.right >= wide.area.right - 1, `the Tool log at the left, the column at the right: ${JSON.stringify(wide)}`);
      assert.ok(wide.log.bottom >= wide.area.bottom - 2 && wide.viewer.top <= wide.area.top + 2, `the Tool log fills its panel's height: ${JSON.stringify(wide)}`);
      assert.deepEqual(wide.sections, ["Tools", "Result", "Debug"], "the column holds the Tools, the Result and Debug");
      assert.ok(wide.instruction && wide.tools && wide.result && wide.side
        && wide.instruction.left >= wide.side.left - 1 && wide.instruction.right <= wide.side.right + 1
        && wide.instruction.bottom <= wide.tools.top + 1 && wide.tools.bottom <= wide.result.top + 1,
      `the instruction stands in the right column above the Tools and the Result: ${JSON.stringify(wide)}`);
      assert.ok(Math.abs(wide.side.right - wide.side.left - wideSizes.detail) <= 2,
        `the column at the right is as wide as the Detail: ${JSON.stringify({ side: wide.side, detail: wideSizes.detail })}`);
      assert.ok(wide.actionsLast, "the row of actions ends the Result");
      // While a tool runs, its time and Cancel stand between Release claim and Submit; Cancel
      // stops it, the review and its draft staying.
      await fill(browser, "#field-comment", "draft kept through the tool");
      await browser.$('[aria-label="Human review"]').$("button=slow").click();
      const row = () => browser.execute(() => {
        const result = document.querySelector<HTMLElement>('[aria-label="Human review"] section[aria-label="Result"]');
        const actions = result?.querySelector(".actions");
        const at = (selector: string) => {
          const control = actions?.querySelector(selector);
          const box = control?.getBoundingClientRect();
          if (!box || !box.width || !box.height || !control?.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2))) return null;
          return { left: box.left, right: box.right, middle: box.y + box.height / 2 };
        };
        const boxes = [at("button[aria-label^='Release claim']"), at(".busy .seconds"), at("button[aria-label^='Cancel']"), at("button.primary")];
        const form = result?.querySelector("#field-comment")?.getBoundingClientRect();
        const sameRow = !!form && boxes.every((box, index) => box && box.middle >= form.bottom
          && Math.abs(box.middle - boxes[0]!.middle) <= 2 && (index === 0 || boxes[index - 1]!.right <= box.left));
        return { sameRow, boxes, seconds: actions?.querySelector(".busy .seconds")?.textContent?.trim() ?? "" };
      });
      let running = await row();
      await browser.waitUntil(async () => {
        running = await row();
        return running.sameRow && /^\d+s$/.test(running.seconds);
      }, { timeout: 15000, timeoutMsg: "Release claim, seconds, Cancel and Submit share one visible row beneath the Result" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(running)}`); });
      await waitFor(browser, async () => await slowTool(fixture)?.() === true, "the owned slow tool is listening before Cancel");
      const alive = slowTool(fixture)!;
      const review = browser.$('[aria-label="Human review"]');
      const release = review.$('button[aria-label="Release claim (u)"]');
      assert.equal(await release.isEnabled(), false, "Release claim cannot be clicked while the tool runs");
      await review.$('[aria-label="Instruction"]').click();
      const releaseCalls = async () => (await checkpoint(browser)).activity.commands.humanRelease?.started ?? 0;
      const beforeBusyShortcut = await releaseCalls();
      await press(browser, ["u"], () => true, "the release shortcut reached the busy review");
      assert.ok(await alive() && /^\d+s$/.test((await row()).seconds), "the gated tool still runs");
      assert.equal(await review.$(".stage").getText(), "REVIEW (yours)", "the review still shows its claim");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "the release attempt kept the claim");
      await review.$('button[aria-label="Cancel (Esc)"]').click();
      await waitFor(browser, async () => !(await alive()) && (await row()).seconds === "", "Cancel stopped the owned tool and dismissed its timer");
      await commandsRendered(browser);
      assert.equal(await releaseCalls(), beforeBusyShortcut, "the busy shortcut never sent a release, including one with a delayed write");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "the claim remains once the tool's cancellation completed");
      assert.equal(await browser.$("#field-comment").getValue(), "draft kept through the tool", "the draft stays");
      // Narrow, a Main area under twice the Detail's width: one column, top to bottom, the Tool
      // log a box between the Tools and the Result.
      await browser.setWindowSize(800, 760);
      await waitFor(browser, async () => oneColumn(await arranged()), "the instruction, Tools, Tool log and Result have one readable column in the narrow Main area");
      const narrow = await arranged();
      assert.deepEqual(narrow.sections, ["Tools", "Tool log", "Result", "Debug"], "one column, the Tool log between the Tools and the Result");
      assert.ok(narrow.actionsLast, "the row of actions still ends the Result");
      assert.equal(await browser.$("#field-comment").isExisting(), true, "the form kept through the change");
      // A wide window gives the Run plenty of room, but its Human eval tab is still one column.
      await browser.setWindowSize(1920, 1080);
      await showRun(browser, "repo  master", run);
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await waitFor(browser, async () => await browser.$('aside[aria-label="Detail"] [aria-label="Human review"] [aria-label="Instruction"]').isDisplayed() && oneColumn(await arranged()),
        "the Run Detail has the instruction above Tools, Tool log and Result in one readable column");
      await checkHeadingAndDebug('aside[aria-label="Detail"]', 'aside[aria-label="Detail"] header h2', false);
      // The Main area's review left the page with the Run's graph; focus the Detail's own.
      await browser.$('aside[aria-label="Detail"] [aria-label="Human review"] [aria-label="Instruction"]').click();
      await press(browser, ["u"], async () => signoff(fixture).claim === null && await claimable(browser), "the same shortcut releases the idle review");
      assert.equal(await releaseCalls(), beforeBusyShortcut + 1, "the release probe detects the successful shortcut");
      const inDock = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"]');
      await inDock().$("button=Claim to review").click();
      await waitFor(browser, () => reviewing(browser), "the review is claimed again");
      await inDock().$("button=GREEN").click();
      const approved = "ready after cancelling the tool";
      await fill(browser, "#field-comment", approved);
      await inDock().$("button=slow").click();
      const submit = () => inDock().$("button=Submit GREEN");
      await waitFor(browser, async () => await slowTool(fixture)?.() === true && await submit().isEnabled(), "Submit stays usable with the tool running");
      const activeTool = slowTool(fixture)!;
      const gate = nativePath(fixture.root, "submit.gate");
      const fingerprints = () => readText(nativePath(fixture.root, "submit.runs")).split("\n").filter(Boolean).length;
      const beforeSubmit = fingerprints();
      removeTree(gate);
      await submit().click();
      await waitFor(browser, () => fingerprints() > beforeSubmit, "Submit reaches the gated fingerprint check");
      assert.equal(await activeTool(), false, "the tool has already exited when submission starts");
      assert.equal(signoff(fixture).result, null, "the gated submit has not saved yet");
      assert.equal(await inDock().$('button[aria-label^="Release claim"]').isEnabled(), false, "Release remains disabled while submitting");
      assert.equal(await submit().isEnabled(), false, "Submit is disabled while submitting");
      const toolsDisabled = await browser.execute((selector) => {
        const buttons = [...document.querySelectorAll<HTMLButtonElement>(`${selector} section[aria-label="Tools"] button`)];
        return buttons.length > 0 && buttons.every((button) => button.disabled);
      }, 'aside[aria-label="Detail"] [aria-label="Human review"]');
      assert.ok(toolsDisabled, "every tool button is disabled while the submission is pending");
      writeText(gate, "go");
      const completed = () => inDock().$('section[aria-label="Result"] pre');
      await waitFor(browser, async () => !(await activeTool()) && signoff(fixture).status === "GREEN"
        && await completed().isDisplayed() && (await completed().getText()).includes(approved), "the tool stopped and the form was submitted");
    },
  ),
];
