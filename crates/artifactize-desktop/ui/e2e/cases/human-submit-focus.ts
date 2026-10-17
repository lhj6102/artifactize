// A submitted Human Detail stays open, but its removed form must not strand keyboard
// focus: the next Escape leaves it in both the Monitor's Run graph and the Review screen.
import assert from "node:assert/strict";
import { create, signoff } from "../fixture.ts";
import { claimable, reviewing } from "../human-review.ts";
import { saveEvidence } from "../observation.ts";
import { fill, press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";
import { graphShown, openArtifact, openTab } from "../graph-view.ts";

export const scenarios: Scenario[] = ["Monitor graph", "Review"].map((screen) =>
  // UX edges: E-HUMAN-GREEN E-HUMAN-SUBMIT E-HUMAN-ESC-DETAIL
  scenario<{ screen: string }>(
    `human-submit-focus-${screen === "Review" ? "review" : "monitor-graph"}`,
    `leaves the completed Human Detail with Escape after GREEN submission in ${screen}`,
    async () => ({ fixture: await create(), args: screen === "Review" ? ["review"] : [], data: { screen } }),
    async (browser, { fixture, data: { screen } }) => {
      const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
      const text = () => browser.execute(() => document.body.innerText);
      if (screen === "Monitor graph") {
        // A click on release, then on its sign-off's tab, shows the Human Detail.
        await graphShown(browser, "a:release");
        await openArtifact(browser, "a:release", "Artifact release");
        await openTab(browser, "e:release/signoff");
        await human().waitForExist({ timeout: 15000, timeoutMsg: "the sign-off's Human Detail" });
        await human().$("button*=Claim").waitForEnabled({ timeout: 15000, timeoutMsg: "the review ready to claim" });
        await human().click();
      } else {
        await browser.$('[aria-label="Requests"]').waitForExist({ timeout: 15000 });
        await press(browser, [primaryModifier, "o"], () => browser.$('[aria-label="Request ID"]').isExisting(), "open by ID");
        await fill(browser, '[aria-label="Request ID"]', signoff(fixture).id);
        await browser.$('[role="dialog"] button[type="submit"]').click();
        await human().waitForExist({ timeout: 15000 });
      }
      await browser.waitUntil(() => claimable(browser), { timeout: 15000, timeoutMsg: "the claim stage" });
      await press(browser, ["c"], () => reviewing(browser), "claimed Human Detail");
      assert.equal(signoff(fixture).claim?.reviewer, "alice");
      await press(browser, ["g"], () => browser.execute(() => document.activeElement?.id === "field-comment"), "the focused GREEN comment");
      await fill(browser, "#field-comment", "checked before closing");
      await press(browser, [primaryModifier, "s"], async () =>
        signoff(fixture).status === "GREEN" && /completed result/i.test(await text()), "the saved, completed Detail");
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "checked before closing" });
      assert.equal(await browser.$("#field-comment").isExisting(), false, "the completed result replaced the form");
      assert.ok(await human().isExisting(), "submission leaves the completed Detail open");
      await saveEvidence(browser, fixture, `${screen}-completed`);
      // No click or scripted focus between submitting from the field and pressing Escape.
      await press(browser, ["Escape"], async () => !(await human().isExisting()), "leaving the completed Detail");
      await saveEvidence(browser, fixture, `${screen}-closed`);
    },
  ),
);
