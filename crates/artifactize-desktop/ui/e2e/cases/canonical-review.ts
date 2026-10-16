// The real IPC resolves both request IDs to one review and one kept draft, in either order.
import assert from "node:assert/strict";
import { artifactize, create, requests, verifyWaiting } from "../fixture.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [false, true].map((followerFirst) =>
  // UX edges: E-REVIEW-OPEN-ID E-HUMAN-ESC-FORM E-HUMAN-SWITCH
  scenario<{ ids: string[] }>(
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
      return { fixture, args: ["review"], data: { ids } };
    },
    async (browser, { fixture, data: { ids } }) => {
      const human = () => browser.$('[aria-label="Human review"]');
      const field = () => browser.$("#field-comment");
      await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });

      async function open(id: string) {
        await press(browser, ["Control", "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the request ID dialog");
        await browser.$('[aria-label="Request ID"]').setValue(id);
        await browser.$('[role="dialog"] button[type="submit"]').click();
        await browser.waitUntil(
          async () => !(await browser.$('[aria-label="Request ID"]').isExisting()) && await human().isExisting(),
          { timeout: 15000, timeoutMsg: "the resolved review opens" },
        );
        await browser.waitUntil(() => browser.execute(() =>
          document.querySelector('[aria-label="Human review"]')?.textContent?.includes("REVIEW (yours)") ?? false), { timeout: 15000 });
        await human().click();
        await press(browser, ["g"], () => field().isExisting(), "the kept GREEN form");
      }
      async function close() {
        await press(browser, ["Escape"], async () => !(await field().isExisting()), "the form closes with its draft kept");
        await press(browser, ["Escape"], async () => !(await human().isExisting()), "the Detail closes");
      }

      await open(ids[0]);
      await field().setValue("one canonical draft");
      await browser.waitUntil(async () => (await field().getValue()) === "one canonical draft");
      await close();
      await open(ids[1]);
      assert.equal(await field().getValue(), "one canonical draft");
      await field().setValue("edited through the other ID");
      await browser.waitUntil(async () => (await field().getValue()) === "edited through the other ID");
      await close();
      await open(ids[0]);
      assert.equal(await field().getValue(), "edited through the other ID");
      assert.ok(requests(fixture).filter((request) => request.evalId === "release/signoff").every((request) => request.result === null),
        "draft editing submitted no Human verdict");
    },
  ),
);
