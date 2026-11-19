// A Human review, in a graph's Detail and in the main area: its claim, its layout, its tools and
// their Tool log, its GREEN and RED forms, text in any language, and stepping back out of it.
import assert from "node:assert/strict";
import { artifactize, createRelease, request } from "../fixtures/repo.ts";
import { nativePath, writeText } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "human-claim",
    title: "a waiting review opened from its Artifact's Detail reads instruction, Tools, Tool log, Result and Debug; another reviewer's claim shows as taken, and Claim to review and Release claim claim and release it, the draft kept",
    journeys: ["J-HUMAN-CLAIM", "J-HUMAN-LAYOUT"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: { id: request(fixture).id } };
    },
    run: async (app, { fixture, data: { id } }) => {
      const review = app.detailReview;
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      const shown = await review.until((each) => each.stage === "CLAIM", "the review opens at its claim stage");
      assert.deepEqual(shown.sections, ["Instruction", "Tools", "Tool log", "Result", "Debug"], "one order");
      assert.equal(await review.instruction(), "Read the release notes and approve them.");
      assert.match(await review.debugState(), new RegExp(`^Request ${id} · DEFERRED · unclaimed`), "its facts under Debug");
      // Another reviewer's claim shows as taken, the review read-only.
      assert.equal(artifactize(fixture, ["request", "claim", id, "--reviewer", "bob"]).status, 0);
      await review.until((each) => each.claimant === "bob" && each.readOnly, "bob's claim shows, read-only");
      assert.equal(artifactize(fixture, ["request", "unclaim", id, "--reviewer", "bob"]).status, 0);
      await review.until((each) => each.stage === "CLAIM" && each.claimant === "", "released by bob, it waits for a claim again");
      await review.claim();
      assert.equal(request(fixture).claim?.reviewer, "alice");
      // Released while its form is being edited, the draft stays for the next claim.
      await review.choose("GREEN");
      await review.enter("comment", "half-checked");
      await review.release();
      assert.equal(request(fixture).claim, null);
      await review.claim();
      await review.choose("GREEN");
      assert.equal(await review.value("comment"), "half-checked", "the draft kept");
      assert.equal(request(fixture).status, "DEFERRED", "nothing submitted");
    },
  }),

  scenario({
    id: "human-tools",
    title: "the review's tools run once it is claimed, each run logged as it happens: its start, its lines with stderr in red, and its end, red when it failed or could not run; a launched application's lines keep arriving under its run",
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
      await review.runTool("notes");
      assert.deepEqual(await review.runUntil("notes", (rows) => rows.at(-1)?.startsWith("■") === true, "notes ended"),
        ["▶ HH:MM:SS · notes · release", "# Release notes", "- Ready to ship.", "■ exit 0"]);
      await review.runTool("fail");
      assert.deepEqual(await review.runUntil("fail", (rows) => rows.at(-1)?.startsWith("■") === true, "fail ended"),
        ["▶ HH:MM:SS · fail · release", "■ exit 1 [red]"]);
      assert.equal((await review.read())?.notice, "fail_release failed; a tool error is not a verdict.");
      await review.runTool("gone");
      const gone = await review.runUntil("gone", (rows) => rows.at(-1)?.startsWith("■") === true, "the missing tool ended");
      assert.match(gone.at(-1)!, /^■ error: \S.* \[red\]$/, "a tool that could not run ends with its error, in red");
      // A launched application keeps logging under its own run after its button returned.
      await review.runTool("talk");
      await review.runUntil("talk", (rows) => rows.includes("opened the stand-in"), "the launched application's first line");
      writeText(gate, "go");
      const talk = await review.runUntil("talk", (rows) => rows.at(-1)?.startsWith("■") === true, "the launched application ended");
      assert.deepEqual(talk.filter((row) => !row.endsWith("[red]")), ["▶ HH:MM:SS · talk · release", "opened the stand-in", "chose: B", "■ exit 0"]);
      assert.deepEqual(talk.filter((row) => row.endsWith("[red]")), ["warning: B is slower [red]"], "its stderr in red");
      assert.equal(request(fixture).status, "DEFERRED", "tools are no verdict");
    },
  }),

  scenario({
    id: "human-green",
    title: "after a claim GREEN opens the pass form, its required fields marked and its optional ones saying Optional, takes Korean composed by an input method and typed letters as text, and Submit saves it, the completed result staying open",
    journeys: ["J-HUMAN-GREEN", "J-HUMAN-FORM", "J-HUMAN-TEXT"],
    prepare: async (own) => {
      const fixture = await createRelease(own, { scalars: true });
      return { fixture, args: ["review"], data: null };
    },
    run: async (app, { fixture }) => {
      const review = app.review;
      await review.claim();
      await review.choose("GREEN");
      const fields = await review.fields();
      assert.deepEqual(fields.map((field) => field.label), ["channel", "comment*", "confirmed", "note", "score"], "the pass schema's fields, its constant left out");
      assert.ok(fields.filter((field) => ["score", "note", "channel"].includes(field.label)).every((field) => field.hint.includes("Optional")),
        `an empty optional field says Optional: ${JSON.stringify(fields)}`);
      // Korean composed by an input method, then letters that are shortcuts outside a field.
      await review.compose("comment", "확인");
      await review.typeInto("comment", " gu");
      assert.equal(await review.value("comment"), "확인 gu", "typed letters are text, not shortcuts");
      assert.equal(request(fixture).claim?.reviewer, "alice", "u in a field released nothing");
      await review.enter("score", "4.5");
      await review.check("confirmed", "true");
      await review.select("channel", "\"preview\"");
      const done = await review.submit();
      assert.match(done.completed, /확인 gu/, "the completed result stays open");
      const saved = request(fixture);
      assert.equal(saved.status, "GREEN");
      assert.deepEqual(saved.result, { verdict: "GREEN", approved: true, comment: "확인 gu", score: 4.5, confirmed: true, channel: "preview" });
    },
  }),

  scenario({
    id: "human-red",
    title: "RED opens the fail form; a result failing its schema is not submitted and names the failing field, GREEN and RED switch keeping each draft, and lists and groups of fields are added and removed before Submit",
    journeys: ["J-HUMAN-RED", "J-HUMAN-FORM"],
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
      await review.enter("reason", "no");
      const refused = await review.submit();
      assert.match(refused.refusal, /reason/, "the failing field is named");
      assert.equal(request(fixture).status, "DEFERRED", "nothing submitted");
      await review.choose("GREEN");
      await review.enter("comment", "checked");
      await review.choose("RED");
      assert.equal(await review.value("reason"), "no", "the RED draft kept");
      await review.enter("reason", "the notes are wrong");
      // A list of steps, one text each, and repeated groups of findings.
      await review.add("steps");
      await review.enterItem("steps", 0, "open the notes");
      // One group of findings is always left to type in; another is added and removed.
      await review.add("findings");
      await review.enter("findings-0-problem", "a typo");
      await review.select("findings-0-severity", "\"low\"");
      await review.remove("findings", 1);
      await review.submit();
      const saved = request(fixture);
      assert.equal(saved.status, "RED");
      assert.deepEqual(saved.result, { verdict: "RED", reason: "the notes are wrong", steps: ["open the notes"], findings: [{ problem: "a typo", severity: "low" }] });
    },
  }),

  scenario({
    id: "human-esc",
    title: "Cancel beside running work cancels it; in the Detail, Close first cancels the review's running work, then leaves its form with the draft kept, and only then closes the Detail",
    journeys: ["J-HUMAN-ESC"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      const review = app.detailReview;
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      await review.claim();
      await review.choose("GREEN");
      await review.enter("comment", "kept while stepping back");
      await review.runTool("slow");
      await review.until((each) => each.running !== "", "the tool runs");
      await review.cancel();
      await review.runUntil("slow", (rows) => rows.at(-1)?.startsWith("■ cancelled") === true, "Cancel ended the run as cancelled");
      // Close steps back one thing at a time.
      await review.runTool("slow");
      await review.until((each) => each.running !== "", "the tool runs again");
      await app.detail.close();
      await review.runUntil("slow", (rows) => rows.at(-1)?.startsWith("■ cancelled") === true, "Close first cancelled the running tool");
      assert.equal(await review.value("comment"), "kept while stepping back", "the form still open");
      await app.detail.close();
      await review.until((each) => each.verdict === "", "Close then left the form");
      assert.ok(await app.detail.read(), "the Detail still open");
      await app.detail.close();
      await app.detail.closed();
      await app.graph.open("a:release", "Artifact release");
      await app.detail.openEval("release/signoff");
      await review.choose("GREEN");
      assert.equal(await review.value("comment"), "kept while stepping back", "the draft kept");
    },
  }),
];
