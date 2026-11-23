// Mode references as a person follows them across Verify, Review and Chat.
import assert from "node:assert/strict";
import { CHAT_ANSWER, CHAT_GATE, CHAT_MODEL, DEFAULT_MODEL, NEXT_DELTA, CONTINUE_ANSWER, CONTINUE_PROMPT, installChatProvider, PARTIAL_ANSWER, QUIT_PROMPT, STOP_PROMPT, STOP_STREAM_CLOSED, WORK_PROMPT } from "../fixtures/chat.ts";
import { runtimeProfile } from "../fixtures/declarations.ts";
import { addRepository, artifactize, createRelease, request, requests } from "../fixtures/repo.ts";
import { exists, nativePath, readText, writeText } from "../portable.ts";
import { portablePath } from "../platform.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { windows } from "../views/viewers.ts";

export const scenarios: Scenario[] = [
  scenario<{ id: string; runId: string; otherId: string; otherRun: string }>({
    id: "review-reference",
    title: "Claim from Verify; open the claimed review outside the default filter, keep its draft on release and restart, submit it and go back to its Run and Artifact",
    journeys: ["J-DETAIL-REVIEW", "J-REVIEW-REF", "J-REVIEW-BACK", "J-HUMAN-DRAFT", "J-PANE-ROWS"],
    prepare: async (owned) => {
      const fixture = await createRelease(owned);
      const { id, runId } = request(fixture);
      await addRepository(fixture, "other");
      const other = requests(fixture).find((saved) => saved.evalId === "release/signoff" && saved.id !== id)!;
      assert.notEqual(other.runId, runId);
      return { fixture, args: ["--repo", fixture.repo], data: { id, runId, otherId: other.id, otherRun: other.runId } };
    },
    run: async (app, { fixture, data: { id, runId, otherId } }) => {
      await app.graph.titled(runId);
      await app.graph.open("a:release", "Artifact release");
      const stage = (shown: string) => app.detail.until((detail) => detail.evals.find((row) => row.id === "e:release/signoff")?.stage === shown, `the sign-off's row reads ${shown}`);
      await stage("claimable");
      // Another reviewer's claim shows on the row, named; released, the review is claimable again.
      assert.equal(artifactize(fixture, ["request", "claim", id, "--reviewer", "bob"]).status, 0);
      await stage("taken by bob");
      assert.equal(artifactize(fixture, ["request", "unclaim", id, "--reviewer", "bob"]).status, 0);
      await stage("claimable");
      await app.detail.reviewAction("Claim to review");
      await stage("yours");
      await app.waitingBadge(1);
      await app.mode("Review");
      const listed = async () => (await app.review.list()).map((row) => row.id);
      await app.until(async () => (await listed()).join() === otherId, "the waiting filter lists only the other review");
      await app.mode("Verify");
      await app.detail.reviewAction("Open in Review");
      await app.review.titleContains("release/signoff");
      await app.review.until((shown) => shown.stage === "Yours", "the direct reference shows the claimed review");
      assert.equal(await app.review.hasOutsideHint(), false);
      assert.equal(await app.review.filter(), "type:human waiting");
      assert.deepEqual(await listed(), [otherId], "opening the exact review did not pin it into the filtered list");
      assert.match(await app.review.debugState(), new RegExp(`^Request ${id} `));
      await app.review.choose("GREEN");
      await app.review.enter("comment", "Release notes checked");
      await app.review.choose("RED");
      await app.review.enter("reason", "Check the linked notes again");
      await app.review.release();
      await app.waitingBadge(2);
    },
    restarted: async (app, { fixture, data: { id, runId, otherRun } }) => {
      await app.mode("Verify");
      await app.pane.selectBranch("other  master");
      await app.graph.titled(otherRun);
      await app.mode("Review");
      await app.review.openListed(id, true);
      await app.review.claim();
      await app.review.until((shown) => shown.verdict === "RED", "Review automatically opens the saved RED verdict after restart");
      assert.equal(await app.review.value("reason"), "Check the linked notes again");
      // Without any new verdict selection, the pet also opens the draft's last verdict.
      const w = windows();
      await w.mode("Verify");
      await w.pet();
      await w.petUntil((shown) => shown.verdict === "RED", "the pet opens the persisted RED verdict after restart");
      await w.mode("Review");
      await app.review.choose("GREEN");
      await app.until(async () => (await app.review.value("comment")) === "Release notes checked", "reclaim after restart restored the GREEN draft");
      const completed = await app.review.submit();
      assert.match(completed.completed, /Release notes checked/);
      const saved = requests(fixture).find((row) => row.id === id);
      assert.equal(saved?.status, "GREEN");
      await app.waitingBadge(1);
      await app.review.back();
      await app.graph.titled(runId);
      await app.detail.until((detail) => detail.tabs.some((tab) => tab.name === "signoff" && tab.selected), "the originating Artifact Detail is open on its sign-off");
    },
  }),
  scenario({
    id: "chat-verify-stop",
    title: "New Chat uses a listed fixture model, reads and verifies, opens the Run and returns by the origin chip, then stops an in-flight turn",
    journeys: ["J-BAR", "J-MODES", "J-CHAT", "J-CHAT-NEW", "J-CHAT-MODEL", "J-CHAT-SEND", "J-CHAT-RUN", "J-CHAT-STOP", "J-RUN-ORIGIN", "J-CHAT-READ", "J-CHAT-GRAPH", "J-PANE-ROWS"],
    prepare: async (owned) => {
      const fixture = await createRelease(owned);
      await installChatProvider(owned, fixture);
      const gate = nativePath(fixture.root, "chat-run-gate");
      const definition = nativePath(fixture.repo, ".artifactize", "docs.artf");
      writeText(definition, readText(definition).replace(/^profile = .*$/m,
        `profile = ${runtimeProfile("slow", 120000, portablePath(gate))}`));
      return { fixture, args: ["--repo", fixture.repo], data: { firstRun: request(fixture).runId, gate } };
    },
    run: async (app, { fixture, data: { firstRun, gate } }) => {
      await app.graph.titled(firstRun);
      const bar = await app.bottomBar();
      await app.mode("Review");
      assert.equal(await app.bottomBar(), bar, "Review keeps the shared bar in place");
      await app.mode("Chat");
      assert.equal(await app.bottomBar(), bar, "Chat keeps the shared bar in place");
      // The branch has no session yet: it starts on the first model listed, not the one chosen
      // next, which the fixture provider lists second.
      const starting = await app.chat.create();
      assert.match(starting, /^\S.* · \S+$/, "the dialog names the starting provider and model");
      assert.notEqual(starting, `fixture · ${CHAT_MODEL}`, "the session does not start on the model chosen next");
      // Only the fixture provider answers, and it refuses a turn on any other model than this.
      await app.chat.model(CHAT_MODEL, DEFAULT_MODEL);
      await app.chat.send(WORK_PROMPT);
      await app.chat.says("release/notes.md");
      const run = await app.chat.runCard();
      assert.notEqual(run, firstRun);
      await app.chat.runStatus(run, "Running");
      writeText(gate, "finish the live eval");
      await app.chat.says(CHAT_ANSWER);
      await app.chat.runStatus(run, "GREEN");
      await app.until(async () => { const where = await app.chat.reading(run); return where.atEnd && !where.reference; }, "the completed turn follows its long answer beyond the Run reference");
      await app.mode("Verify");
      await app.graph.titled(firstRun);
      await app.mode("Chat");
      await app.chat.openRun(run);
      await app.graph.titled(run);
      await app.pane.branch("repo  master", (branch) => branch.expanded && branch.entries.some((entry) => entry.id === run && entry.shown), "Open in Verify expanded the branch and selected this Run in its timeline");
      await app.mode("Chat");
      // A new session on the branch starts with the model its last session used.
      assert.equal(await app.chat.create(), `fixture · ${CHAT_MODEL}`);
      assert.equal(await app.chat.selected(), CHAT_MODEL);
      await app.chat.emptySession();
      await app.mode("Verify");
      await app.graph.titled(run);
      await app.graph.openOrigin(WORK_PROMPT);
      await app.until(async () => { const where = await app.chat.reading(run); return where.reference && !where.atEnd; }, "the origin opens at the Run reference, not the conversation's end");
      await app.chat.scrollToEnd(run);
      await app.chat.send(STOP_PROMPT);
      await app.chat.says(PARTIAL_ANSWER);
      await app.chat.working();
      await app.chat.scrollUp(run);
      const reading = await app.chat.reading(run);
      writeText(nativePath(fixture.root, CHAT_GATE), "continue streaming");
      await app.chat.says(NEXT_DELTA);
      const streamed = await app.chat.reading(run);
      assert.equal(streamed.top, reading.top, "a new streamed line does not move what is being read");
      assert.equal(streamed.atEnd, false);
      await app.chat.scrollToEnd(run);
      const closed = nativePath(fixture.root, STOP_STREAM_CLOSED);
      assert.equal(exists(closed), false, "the provider is still streaming before Stop");
      await app.chat.stop(PARTIAL_ANSWER);
      await app.eventually(() => exists(closed), "Stop closed the provider's unfinished response stream");
      await app.chat.send(CONTINUE_PROMPT);
      await app.chat.says(CONTINUE_ANSWER);
      await app.chat.show("Graph");
    },
    restarted: async (app) => {
      await app.mode("Chat");
      await app.chat.sessionsUntil([WORK_PROMPT, "New chat"]);
      await app.chat.filter(WORK_PROMPT);
      await app.chat.sessionsUntil([WORK_PROMPT]);
      await app.chat.openSaved(WORK_PROMPT, true);
      await app.chat.viewShown("Graph");
      await app.chat.show("Conversation");
      await app.chat.says(CONTINUE_ANSWER);
    },
  }),
  scenario({
    id: "chat-quit-while-working",
    title: "the palette's Quit while a Chat turn runs and no claim is held asks first, naming the turn: Stay keeps the turn running, Quit stops it and the app exits",
    journeys: ["J-QUIT"],
    prepare: async (owned) => {
      const fixture = await createRelease(owned);
      await installChatProvider(owned, fixture);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      await app.mode("Chat");
      await app.chat.create();
      await app.chat.model(CHAT_MODEL);
      await app.chat.send(QUIT_PROMPT);
      await app.chat.working();
      await app.command("Quit");
      const asked = await app.dialogTitled("Quit while the agent works?");
      assert.ok(asked.text.includes(`A Chat turn is running in ${QUIT_PROMPT}; quitting stops it.`), asked.text);
      assert.ok(!asked.text.includes("review claim"), "no claim is named");
      await app.dialogButton("Stay");
      await app.dialogClosed();
      await app.command("Quit");
      await app.dialogTitled("Quit while the agent works?");
      await app.exitsBy(() => app.dialogButton("Quit"));
    },
  }),
];
