// Mode references as a person follows them across Verify, Review and Chat.
import assert from "node:assert/strict";
import { CHAT_ANSWER, CHAT_MODEL, installChatProvider, STOP_PROMPT, WORK_PROMPT } from "../fixtures/chat.ts";
import { createRelease, request } from "../fixtures/repo.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "review-reference",
    title: "Claim from Verify; open the claimed review outside the default filter, keep its draft on release, submit it and go back to its Run and Artifact",
    journeys: ["J-DETAIL-REVIEW", "J-REVIEW-REF", "J-REVIEW-BACK", "J-HUMAN-DRAFT", "J-HUMAN-NOTES"],
    prepare: async (owned) => {
      const fixture = await createRelease(owned);
      const { id, runId } = request(fixture);
      return { fixture, args: ["--repo", fixture.repo], data: { id, runId } };
    },
    run: async (app, { fixture, data: { id, runId } }) => {
      await app.graph.open("a:release", "Artifact release");
      await app.detail.reviewAction("Claim to review");
      await app.waitingBadge(0);
      await app.detail.reviewAction("Open in Review");
      await app.review.titleContains("release/signoff");
      await app.until(async () => /outside the filter.*Claimed by you/i.test(await app.review.outside()), "the direct reference explains the claim");
      assert.equal(await app.review.filter(), "type:human waiting");
      assert.match(await app.review.debugState(), new RegExp(`^Request ${id} `));
      await app.review.writeNotes("Keep the checked selector.");
      await app.review.choose("GREEN");
      await app.review.enter("comment", "Release notes checked");
      await app.review.release();
      await app.waitingBadge(1);
      await app.review.claim();
      await app.review.choose("GREEN");
      await app.until(async () => (await app.review.notes()) === "Keep the checked selector." && (await app.review.value("comment")) === "Release notes checked", "reclaim restored the Notes and GREEN draft");
      const completed = await app.review.submit();
      assert.match(completed.completed, /Keep the checked selector\./);
      const saved = request(fixture);
      assert.equal(saved.status, "GREEN");
      await app.review.back();
      await app.graph.titled(runId);
      await app.detail.until((detail) => detail.tabs.some((tab) => tab.name === "signoff" && tab.selected), "the originating Artifact Detail is open on its sign-off");
    },
  }),
  scenario({
    id: "chat-verify-stop",
    title: "New Chat uses a listed fixture model, reads and verifies, opens the Run and returns by the origin chip, then stops an in-flight turn",
    journeys: ["J-BAR", "J-MODES", "J-CHAT", "J-CHAT-NEW", "J-CHAT-MODEL", "J-CHAT-SEND", "J-CHAT-RUN", "J-CHAT-STOP", "J-RUN-ORIGIN"],
    prepare: async (owned) => {
      const fixture = await createRelease(owned);
      await installChatProvider(owned, fixture);
      return { fixture, args: ["--repo", fixture.repo], data: { firstRun: request(fixture).runId } };
    },
    run: async (app, { data: { firstRun } }) => {
      await app.graph.titled(firstRun);
      await app.mode("Chat");
      await app.chat.create(CHAT_MODEL);
      await app.chat.model(CHAT_MODEL);
      await app.chat.send(WORK_PROMPT);
      await app.chat.says("release/notes.md");
      const run = await app.chat.runCard();
      assert.notEqual(run, firstRun);
      await app.chat.says(CHAT_ANSWER);
      await app.chat.runStatus(run, "GREEN");
      await app.mode("Verify");
      await app.graph.titled(firstRun);
      await app.mode("Chat");
      await app.chat.openRun(run);
      await app.graph.titled(run);
      await app.mode("Chat");
      await app.chat.create(CHAT_MODEL);
      await app.chat.emptySession();
      await app.mode("Verify");
      await app.graph.titled(run);
      await app.graph.openOrigin(WORK_PROMPT);
      await app.chat.says(CHAT_ANSWER);
      await app.chat.send(STOP_PROMPT);
      await app.chat.stop();
      await app.chat.says(WORK_PROMPT);
    },
  }),
];
