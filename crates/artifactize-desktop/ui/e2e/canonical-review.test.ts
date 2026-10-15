// The real IPC resolves both request IDs to one review and one kept draft, in either order.
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { artifactize, create, requests, verifyWaiting, type Fixture } from "./fixture.ts";
import { checkBuild, start, type Session } from "./driver.ts";
import { press } from "./keys.ts";

for (const followerFirst of [false, true]) {
  describe(`canonical Review (${followerFirst ? "follower" : "original"} first)`, () => {
    let fixture: Fixture;
    let session: Session;
    let browser: WebdriverIO.Browser;
    let ids: string[];
    const human = () => browser.$('[aria-label="Human review"]');
    const field = () => browser.$("#field-comment");

    beforeEach(async () => {
      checkBuild();
      fixture = await create();
      await verifyWaiting(fixture);
      const signoffs = requests(fixture).filter((request) => request.evalId === "release/signoff");
      const original = signoffs.find((request) =>
        (request as unknown as { execution?: { provenance?: { requestId?: string } } }).execution?.provenance?.requestId === request.id);
      assert.ok(original && signoffs.length === 2, "one original and one real follower");
      const follower = signoffs.find((request) => request.id !== original.id)!;
      ids = followerFirst ? [follower.id, original.id] : [original.id, follower.id];
      assert.equal(artifactize(fixture, "request", "claim", original.id, "--reviewer", "alice"), 0);
      session = await start(fixture, "review");
      browser = session.browser;
      await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
    });
    afterEach(async () => { await session?.dispose(); });

    async function open(id: string) {
      await press(browser, ["Control", "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "the request ID dialog");
      await browser.$('[aria-label="Request ID"]').setValue(id);
      await press(browser, ["Enter"], async () =>
        !(await browser.$('[aria-label="Request ID"]').isExisting()) && await human().isExisting(), "the resolved review opens");
      await browser.waitUntil(() => browser.execute(() =>
        document.querySelector('[aria-label="Human review"]')?.textContent?.includes("REVIEW (yours)") ?? false), { timeout: 15000 });
      await human().click();
      await press(browser, ["g"], () => field().isExisting(), "the kept GREEN form");
    }
    async function close() {
      await press(browser, ["Escape"], async () => !(await field().isExisting()), "the form closes with its draft kept");
      await press(browser, ["Escape"], async () => !(await human().isExisting()), "the Detail closes");
    }

    // UX edges: E-REVIEW-OPEN-ID E-HUMAN-ESC-FORM E-HUMAN-SWITCH
    it("keeps the same draft through original/follower/original request openings", async () => {
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
    });
  });
}
