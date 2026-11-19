// The real IPC resolves both request IDs to one review and one kept draft, in either order.
import assert from "node:assert/strict";
import { artifactize, create, requests, savedRuns, verifyWaiting } from "../fixture.ts";
import { reviewing } from "../human-review.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { entriesFocused, expectMainReview, mainTitle, showRun } from "../pane.ts";

export const scenarios: Scenario[] = [false, true].map((followerFirst) =>
  // UX journey: J-OPEN-ID
  scenario<{ ids: string[]; canonical: string; run: string }>(
    `canonical-review-${followerFirst ? "follower" : "original"}-first`,
    `canonical Review (${followerFirst ? "follower" : "original"} first): keeps the same draft through original/follower/original request openings`,
    async () => {
      const fixture = await create();
      const original = requests(fixture).find((request) => request.evalId === "release/signoff");
      assert.ok(original, "the first Run created its waiting request");
      await verifyWaiting(fixture);
      const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
      assert.equal(signoffs.length, 2, "the second Run created a distinct follower");
      const follower = signoffs.find((request) => request.id !== original.id)!;
      const ids = followerFirst ? [follower.id, original.id] : [original.id, follower.id];
      assert.equal(artifactize(fixture, "request", "claim", original.id, "--reviewer", "alice"), 0);
      return { fixture, args: ["review"], data: { ids, canonical: original.id, run: savedRuns(fixture)[0].id } };
    },
    async (browser, { fixture, data: { ids, canonical, run } }) => {
      const human = () => browser.$('[aria-label="Human review"]');
      const field = () => browser.$("#field-comment");
      await browser.waitUntil(async () => (await mainTitle(browser)) === "Review", { timeout: 15000, timeoutMsg: "the newest waiting review open at launch" });

      async function open(id: string) {
        await showRun(browser, "repo  master", run);
        await human().waitForDisplayed({ reverse: true });
        // The palette's Open a request by ID, the journey's visible path.
        await browser.$("header.bar").$("button*=Commands").click();
        await choosePaletteCommand(browser, "Open a request by ID");
        await browser.$('.palette [role="option"][aria-selected="true"]').click();
        await browser.$('[aria-label="Request ID"]').waitForDisplayed({ timeout: 15000, timeoutMsg: "the request ID dialog" });
        await fill(browser, '[aria-label="Request ID"]', id);
        await browser.$('[role="dialog"] button[type="submit"]').click();
        await browser.$('[aria-label="Request ID"]').waitForDisplayed({ reverse: true });
        await expectMainReview(browser, canonical);
        await browser.waitUntil(() => reviewing(browser), { timeout: 15000 });
        await human().click();
        await press(browser, ["g"], () => field().isExisting(), "the kept GREEN form");
      }
      async function close() {
        await press(browser, ["Escape"], async () => !(await field().isExisting()), "the form closes with its draft kept");
        await press(browser, ["Escape"], () => entriesFocused(browser), "the review left for the Pane");
      }

      await open(ids[0]);
      await fill(browser, "#field-comment", "one canonical draft");
      await browser.waitUntil(async () => (await field().getValue()) === "one canonical draft");
      await close();
      await open(ids[1]);
      await browser.waitUntil(async () => await field().getValue() === "one canonical draft",
        { timeout: 15000, timeoutMsg: "the other request ID restores the first draft" });
      await fill(browser, "#field-comment", "edited through the other ID");
      await browser.waitUntil(async () => (await field().getValue()) === "edited through the other ID");
      await close();
      await open(ids[0]);
      await browser.waitUntil(async () => await field().getValue() === "edited through the other ID",
        { timeout: 15000, timeoutMsg: "the original request ID restores the edited draft" });
      assert.ok(requests(fixture).filter((request) => request.evalId === "release/signoff").every((request) => request.result === null),
        "draft editing submitted no Human verdict");
    },
  ),
);
