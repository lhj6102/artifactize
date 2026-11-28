// A Human review as a conversation, in a graph's Detail and in the main area: its claim, its layout,
// its tools and their run lines, its issues, GREEN with none and RED with them, text in any
// language, and stepping back out of it.
import assert from "node:assert/strict";
import { artifactize, createRelease, request } from "../fixtures/repo.ts";
import { processRunning } from "../platform.ts";
import { nativePath, readTextIfExists, writeText, type NativePath } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";
import type { App } from "../views/app.ts";

/** The process of the release's slow tool at its `run`th start, once it has started: the tool
 * records each start's process in `pids`. */
async function slowProcess(app: App, pids: NativePath, run: number): Promise<number> {
  let pid = 0;
  await app.until(async () => {
    pid = Number((readTextIfExists(pids) ?? "").trim().split("\n")[run - 1] ?? 0);
    return pid > 0;
  }, `the tool's process ${run} started`);
  return pid;
}

export const scenarios: Scenario[] = [
  scenario({
    id: "human-claim",
    title: "a waiting review opened from its Artifact's Detail reads as a conversation, its instruction, tools and Debug, one quiet line in the issues box's place and Claim to review in the decision bar; another reviewer's claim leaves only who holds it; Claim to review and Release claim claim and release it, the issues kept",
    journeys: ["J-HUMAN-CLAIM", "J-HUMAN-LAYOUT", "J-HUMAN-ISSUES"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: { id: request(fixture).id } };
    },
    run: async (app, { fixture, data: { id } }) => {
      const review = app.detailReview;
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      const shown = await review.until((each) => each.stage === "Waiting", "the review opens waiting for a claim");
      assert.deepEqual(shown.sections, ["Instruction", "Tools", "Debug", "Claim line", "Decision"], "one conversation, no runs, points or issues before there are some");
      assert.equal(await review.logLine(), "", "no run line before a tool has run");
      assert.equal(await review.instruction(), "Read the release notes and approve them.");
      assert.match(await review.debugState(), new RegExp(`^Request ${id} · DEFERRED · unclaimed`), "its facts under Debug");
      // Another reviewer's claim shows as taken: the bar says only who holds it.
      assert.equal(artifactize(fixture, ["request", "claim", id, "--reviewer", "bob"]).status, 0);
      const taken = await review.until((each) => each.stage === "Taken" && each.claimant === "bob" && each.readOnly, "bob's claim shows as taken, read-only");
      assert.ok(!taken.sections.includes("Issues box") && !taken.sections.includes("Claim line"), "no issues box and no line in its place");
      assert.equal(artifactize(fixture, ["request", "unclaim", id, "--reviewer", "bob"]).status, 0);
      await review.until((each) => each.stage === "Waiting" && each.claimant === "", "released by bob, it waits for a claim again");
      await review.claim();
      assert.equal(request(fixture).claim?.reviewer, "alice");
      assert.ok((await review.read())!.sections.includes("Issues box"), "claimed, the issues box stands in the line's place");
      // Released while issues are written, the draft stays for the next claim.
      await review.addIssue("half-checked");
      await review.release();
      assert.equal(request(fixture).claim, null);
      await review.claim();
      await review.until((each) => each.issues.join() === "half-checked", "the issues kept");
      assert.equal(request(fixture).status, "DEFERRED", "nothing submitted");
    },
  }),

  scenario({
    id: "human-tools",
    title: "the review's tools run once it is claimed, each run its own line in the conversation as it happens: unfolded, its start, its lines with stderr marked with a red rule, and its end; folded, its end and count, a failure in red on its line and nowhere else; a launched application's lines keep arriving under its run",
    journeys: ["J-HUMAN-TOOL", "J-HUMAN-LOG"],
    prepare: async (own) => {
      const fixture = await createRelease(own, { missing: true });
      return { fixture, args: ["review"], data: { gate: nativePath(fixture.root, "talk.gate") } };
    },
    run: async (app, { fixture, data: { gate } }) => {
      const review = app.review;
      const before = await review.until((each) => each.tools.length > 0, "the review's tools listed");
      assert.ok(before.tools.every((tool) => !tool.enabled), "no tool runs before the claim");
      await review.claim();
      await review.runTool("release-tools.notes");
      assert.deepEqual(await review.runUntil("release-tools.notes", (rows) => rows.at(-1)?.startsWith("■") === true, "notes ended"),
        ["▶ HH:MM:SS · release-tools.notes · release", "# Release notes", "- Ready to ship.", "■ exit 0"]);
      await app.until(async () => (await review.logLine()) === "Ran release-tools.notes · release exit 0 · 2 lines", "the run's line shows its end and count");
      await review.runTool("release-tools.fail");
      assert.deepEqual(await review.runUntil("release-tools.fail", (rows) => rows.at(-1)?.startsWith("■") === true, "fail ended"),
        ["▶ HH:MM:SS · release-tools.fail · release", "■ exit 1 [red]"]);
      await app.until(async () => (await review.logLine()) === "Ran release-tools.fail · release exit 1 · 0 lines", "the failed run's line says its end");
      const failed = await review.until((shown) => !shown.running, "the failed run ended");
      assert.equal(failed.notice, "", "a tool's failure is said on its line alone");
      assert.equal((await review.runLines()).length, 2, "each run its own line, the earlier one kept");
      await review.runTool("release-tools.gone");
      const gone = await review.runUntil("release-tools.gone", (rows) => rows.at(-1)?.startsWith("■") === true, "the missing tool ended");
      assert.match(gone.at(-1)!, /^■ error: \S.* \[red\]$/, "a tool that could not run ends with its error, the prefix marked");
      // A launched application keeps logging under its own run after its button returned.
      await review.runTool("release-tools.talk");
      await review.runUntil("release-tools.talk", (rows) => rows.includes("opened the stand-in"), "the launched application's first line");
      writeText(gate, "go");
      const talk = await review.runUntil("release-tools.talk", (rows) => rows.at(-1)?.startsWith("■ exit ") === true, "the launched application ended");
      assert.deepEqual(talk.filter((row) => !row.endsWith("[red]")), ["▶ HH:MM:SS · release-tools.talk · release", "opened the stand-in", "chose: B", "■ exit 0"]);
      assert.deepEqual(talk.filter((row) => row.endsWith("[red]")), ["warning: B is slower [red]"], "its stderr marked with a red rule");
      assert.equal(request(fixture).status, "DEFERRED", "tools are no verdict");
    },
  }),

  scenario({
    id: "human-green",
    title: "after a claim nothing is chosen until GREEN is; GREEN cannot be submitted while an issue is listed, Submit saying why, and can once it is removed; Submit saves GREEN with no issues while a tool runs, the tool stopped first and the result standing as the last bubble",
    journeys: ["J-HUMAN-GREEN", "J-HUMAN-ISSUES"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["review"], data: { pids: nativePath(fixture.root, "slow.gate.pids") } };
    },
    run: async (app, { fixture, data: { pids } }) => {
      const review = app.review;
      await review.claim();
      assert.equal((await review.read())!.why, "Choose GREEN or RED");
      await review.choose("GREEN");
      await review.until((shown) => shown.submittable && shown.why === "", "GREEN with no issue can be submitted");
      // An issue listed, GREEN waits: approval lists none.
      await review.addIssue("the date is wrong");
      const waits = await review.until((shown) => !shown.submittable && shown.why !== "", "Submit says why GREEN waits");
      assert.match(waits.why, /GREEN takes no issues/);
      await review.removeIssue(0);
      await review.until((shown) => shown.submittable, "GREEN can be submitted again");
      // Submit while a tool of the review runs: the submission stops the tool first.
      await review.runTool("release-tools.slow");
      await review.until((each) => each.running !== "", "the tool runs");
      const tool = await slowProcess(app, pids, 1);
      const done = await review.submit();
      assert.equal(processRunning(tool), false, "the tool has already exited when the completed result first appears");
      assert.match(done.completed, /verdict\s*GREEN/, "the result as the last bubble, its verdict first");
      assert.match(done.completed, /issues\s*none/, "with no issue");
      assert.equal(done.stage, "", "submitted, the decision bar is gone");
      assert.ok(!done.sections.includes("Issues box") && !done.sections.includes("Runs"), "the issues box and the runs go");
      const saved = request(fixture);
      assert.equal(saved.status, "GREEN");
      assert.deepEqual(saved.result, { verdict: "GREEN", kind: "human", issues: [] });
    },
  }),

  scenario({
    id: "human-red",
    title: "RED asks an issue before it submits, Submit saying why; the issue field takes Korean composed by an input method and typed letters as text, Submit waiting for the typed issue to be added; issues are edited and removed in place; GREEN and RED switch keeping the issues; RED submits them",
    journeys: ["J-HUMAN-RED", "J-HUMAN-ISSUES", "J-HUMAN-TEXT"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      const review = app.detailReview;
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      await review.claim();
      await review.choose("RED");
      const needs = await review.until((shown) => shown.why !== "" && !shown.submittable, "Submit says why RED waits");
      assert.match(needs.why, /RED needs at least one issue/);
      // Korean composed by an input method, then letters that are shortcuts outside a field.
      await review.compose("확이", "확인");
      await review.typeInto(" gu");
      assert.equal(await review.value(), "확인 gu", "typed letters are text, not shortcuts");
      assert.equal(request(fixture).claim?.reviewer, "alice", "u in a field released nothing");
      await review.until((shown) => shown.why === "Add the typed issue first" && !shown.submittable, "Submit waits for the typed issue");
      await review.addTyped();
      await review.addIssue("open the notes");
      await review.addIssue("a typo");
      await review.editIssue(1, "open the notes first");
      await review.removeIssue(2);
      assert.equal(request(fixture).status, "DEFERRED", "nothing submitted");
      await review.choose("GREEN");
      await review.until((shown) => !shown.submittable && /GREEN takes no issues/.test(shown.why), "GREEN waits while issues are listed");
      await review.choose("RED");
      assert.deepEqual((await review.read())!.issues, ["확인 gu", "open the notes first"], "the issues kept");
      await review.submit();
      const saved = request(fixture);
      assert.equal(saved.status, "RED");
      assert.deepEqual(saved.result, { verdict: "RED", kind: "human", issues: [{ reason: "확인 gu" }, { reason: "open the notes first" }] });
    },
  }),

  scenario({
    id: "human-esc",
    title: "Cancel beside running work cancels it; in the Detail, Close first cancels the review's running work, then closes the Detail with the issues kept",
    journeys: ["J-HUMAN-ESC"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: { pids: nativePath(fixture.root, "slow.gate.pids") } };
    },
    run: async (app, { data: { pids } }) => {
      const review = app.detailReview;
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      await review.claim();
      await review.addIssue("kept while stepping back");
      await review.runTool("release-tools.slow");
      await review.until((each) => each.running !== "", "the tool runs");
      const first = await slowProcess(app, pids, 1);
      await review.cancel();
      await review.runUntil("release-tools.slow", (rows) => rows.at(-1)?.startsWith("■ cancelled") === true, "Cancel ended the run as cancelled");
      await app.until(async () => !processRunning(first), "Cancel stopped the tool's process");
      // Close steps back one thing at a time.
      await review.runTool("release-tools.slow");
      await review.until((each) => each.running !== "", "the tool runs again");
      const second = await slowProcess(app, pids, 2);
      await app.detail.close();
      await review.runUntil("release-tools.slow", (rows) => rows.at(-1)?.startsWith("■ cancelled") === true, "Close first cancelled the running tool");
      await app.until(async () => !processRunning(second), "Close stopped the tool's process");
      assert.ok(await app.detail.read(), "the Detail still open");
      await app.detail.close();
      await app.detail.closed();
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      await review.until((each) => each.issues.join() === "kept while stepping back", "the issues kept");
    },
  }),
];
