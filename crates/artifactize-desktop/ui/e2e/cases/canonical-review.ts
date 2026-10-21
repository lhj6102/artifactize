// The real IPC resolves both request IDs to one review and one kept draft, in either order.
import assert from "node:assert/strict";
import { artifactize, create, requests, verifyWaiting } from "../fixture.ts";
import { reviewing } from "../human-review.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";
import { entriesFocused, mainTitle } from "../pane.ts";
import { checkpoint, commandsRendered } from "../commands.ts";

export const scenarios: Scenario[] = [false, true].map((followerFirst) =>
  // UX journey: J-OPEN-ID; UX edges: E-REVIEW-OPEN-ID E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL
  scenario<{ ids: string[]; canonical: string }>(
    `canonical-review-${followerFirst ? "follower" : "original"}-first`,
    `canonical Review (${followerFirst ? "follower" : "original"} first): keeps the same draft through original/follower/original request openings`,
    async () => {
      const fixture = await create();
      await verifyWaiting(fixture);
      const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
      const original = signoffs.find((request) =>
        (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId === request.id);
      assert.ok(original && signoffs.length === 2, "one original and one real follower");
      const follower = signoffs.find((request) => request.id !== original.id)!;
      const ids = followerFirst ? [follower.id, original.id] : [original.id, follower.id];
      assert.equal(artifactize(fixture, "request", "claim", original.id, "--reviewer", "alice"), 0);
      return { fixture, args: ["review"], data: { ids, canonical: original.id } };
    },
    async (browser, { fixture, data: { ids, canonical } }) => {
      const human = () => browser.$('[aria-label="Human review"]');
      const field = () => browser.$("#field-comment");
      await browser.waitUntil(async () => (await mainTitle(browser)) === "Review", { timeout: 15000, timeoutMsg: "the newest waiting review open at launch" });

      async function open(id: string) {
        await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the request ID dialog");
        await fill(browser, '[aria-label="Request ID"]', id);
        const before = await checkpoint(browser);
        const completed = before.activity.commands.requestDetail?.completed ?? 0;
        await browser.$('[role="dialog"] button[type="submit"]').click();
        // The same review may already show: only this lookup's completion, rendered, resolves it.
        await browser.waitUntil(async () => ((await checkpoint(browser)).activity.commands.requestDetail?.completed ?? 0) === completed + 1,
          { timeout: 15000, timeoutMsg: `the lookup of ${id} completed` });
        await commandsRendered(browser, before);
        await browser.waitUntil(
          async () => !(await browser.$('[aria-label="Request ID"]').isExisting()) && await human().isExisting(),
          { timeout: 15000, timeoutMsg: "the resolved review opens" },
        );
        await browser.waitUntil(async () => (await human().$(".meta").getText()).startsWith(`Request ${canonical} · `),
          { timeout: 15000, timeoutMsg: `${id} resolved to the canonical review ${canonical}` });
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
      assert.equal(await field().getValue(), "one canonical draft");
      await fill(browser, "#field-comment", "edited through the other ID");
      await browser.waitUntil(async () => (await field().getValue()) === "edited through the other ID");
      await close();
      await open(ids[0]);
      assert.equal(await field().getValue(), "edited through the other ID");
      assert.ok(requests(fixture).filter((request) => request.evalId === "release/signoff").every((request) => request.result === null),
        "draft editing submitted no Human verdict");
    },
  ),
);
