// A submitted Human Detail stays open, but its removed form must not strand keyboard
// focus: the next Escape leaves it in both the Monitor's Run graph and the Review screen.
import assert from "node:assert/strict";
import { join } from "node:path";
import { it } from "node:test";
import { create, signoff } from "./fixture.ts";
import { checkBuild, type Session, start } from "./driver.ts";
import { press } from "./keys.ts";

for (const screen of ["Monitor graph", "Review"]) {
  // UX edges: E-HUMAN-GREEN E-HUMAN-SUBMIT E-HUMAN-ESC-DETAIL
  it(`leaves the completed Human Detail with Escape after GREEN submission in ${screen}`, async () => {
    checkBuild();
    const fixture = await create();
    let session: Session | undefined;
    try {
      session = await start(fixture, ...(screen === "Review" ? ["review"] : []));
      const browser = session.browser;
      const human = () => browser.$('[aria-label="Human review"]');
      const text = () => browser.execute(() => document.body.innerText);
      if (screen === "Monitor graph") {
        await browser.$("button*=Graph").waitForExist({ timeout: 15000 });
        await browser.$("button*=Graph").click();
        const release = await browser.$('.svelte-flow__node[data-id="a:release"]');
        await release.waitForExist({ timeout: 15000 });
        await release.click();
        await browser.waitUntil(
          () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"),
          { timeout: 15000, timeoutMsg: "focus in the release evals" },
        );
        await press(browser, ["Enter"], () => human().isExisting(), "the sign-off's Human Detail");
      } else {
        await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
        await press(browser, ["Control", "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "open by ID");
        await browser.$('[aria-label="Request ID"]').setValue(signoff(fixture).id);
        await press(browser, ["Enter"], () => human().isExisting(), "the sign-off's Human Detail");
      }
      await browser.waitUntil(async () => (await text()).includes("Unclaimed. c claims it for alice"), { timeout: 15000 });
      await press(browser, ["c"], async () => (await text()).includes("REVIEW (yours)"), "claimed Human Detail");
      assert.equal(signoff(fixture).claim?.reviewer, "alice");
      await press(browser, ["g"], () => browser.execute(() => document.activeElement?.id === "field-comment"), "the focused GREEN comment");
      await browser.$("#field-comment").setValue("checked before closing");
      await press(browser, ["Control", "s"], async () =>
        signoff(fixture).status === "GREEN" && /completed result/i.test(await text()), "the saved, completed Detail");
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "checked before closing" });
      assert.equal(await browser.$("#field-comment").isExisting(), false, "the completed result replaced the form");
      assert.ok(await human().isExisting(), "submission leaves the completed Detail open");
      if (process.env.HUMAN_FOCUS_EVIDENCE) {
        await browser.saveScreenshot(join(process.env.HUMAN_FOCUS_EVIDENCE, `${screen.replaceAll(" ", "-")}-completed.png`));
      }
      // No click or scripted focus between submitting from the field and pressing Escape.
      await press(browser, ["Escape"], async () => !(await human().isExisting()), "leaving the completed Detail");
      if (process.env.HUMAN_FOCUS_EVIDENCE) {
        await browser.saveScreenshot(join(process.env.HUMAN_FOCUS_EVIDENCE, `${screen.replaceAll(" ", "-")}-closed.png`));
      }
    } catch (error) {
      if (session) {
        const request = signoff(fixture);
        console.log("Human submit failure", screen, { status: request.status, result: request.result }, await session.browser.execute(() => ({
          text: document.body.innerText,
          focused: { tag: document.activeElement?.tagName, id: document.activeElement?.id, label: document.activeElement?.getAttribute("aria-label") },
        })));
        if (process.env.HUMAN_FOCUS_EVIDENCE) {
          await session.browser.saveScreenshot(join(process.env.HUMAN_FOCUS_EVIDENCE, `${screen.replaceAll(" ", "-")}-failure.png`));
        }
      }
      throw error;
    } finally {
      await session?.stop();
    }
  });
}
