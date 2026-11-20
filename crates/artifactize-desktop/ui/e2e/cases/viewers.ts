// Artifact Viewers, Point and the pet overlay: a review's viewer tool opens its Artifact in a
// window of its own; Point appends a place there to the review's Notes; the pet overlay keeps
// the review at hand while its evaluation space is not in front, and submits it.
import assert from "node:assert/strict";
import { request } from "../fixtures/repo.ts";
import { createNotes, createViewed } from "../fixtures/viewers.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { windows } from "../views/viewers.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "viewer-point-pet",
    title: "a review's viewer tool opens the File Explorer in a window of its own; Point appends the line clicked to the review's Notes; away from Review the pet overlay holds the same Notes and submits the review",
    journeys: ["J-VIEWER", "J-VIEWER-FILES", "J-POINT", "J-PET"],
    // Its windows take the display's focus from one another.
    alone: true,
    prepare: async (own) => {
      const fixture = await createNotes(own);
      return { fixture, args: ["review"], data: {} };
    },
    run: async (app, { fixture }) => {
      const w = windows();
      await w.claimReview("notes/signoff");
      await app.review.writeNotes("Check the rounding.");
      const label = await w.runTool("view");
      assert.equal(await w.viewer(label), "notes — File Explorer", "titled {Artifact} — {viewer}");
      const strip = await w.stripUntil((shown) => shown.point, "Point offered for the claimed review");
      assert.deepEqual(strip.items.slice(0, 2), ["notes", "File Explorer"]);
      // Folders first; a folder unfolds by a click; a file shows with its lines.
      const files = await w.entriesUntil("Files", (entries) => entries.includes("release.md"), "the notes' files listed");
      assert.equal(files[0], "drafts", "folders first");
      await w.choose("Files", "drafts");
      await w.entriesUntil("Files", (entries) => entries.includes("old.txt"), "drafts unfolded");
      await w.choose("Files", "release.md");
      const shown = await w.documentUntil((doc) => doc.lines.length >= 3, "release.md shown");
      assert.deepEqual(shown.lines.slice(0, 3), ["# Release", "Totals are rounded.", "Ship it."]);
      // Point, then a click on line 2: its place reaches the review's Notes.
      await w.point();
      await w.clickLine(2);
      const notes = await w.notesUntil((text) => /^Check the rounding\.\nrelease\.md L2:C\d+$/.test(text.trim()), "the pointed line in the Notes");
      // It stands out there as pointed.
      await w.pointedUntil((lines) => lines.length === 1 && lines[0] === notes.trim().split("\n").at(-1), "the pointed line marked in the Notes");
      // Away from Review the pet overlay holds the review, its Notes the same draft's.
      await w.mode("Verify");
      const pet = await w.pet();
      assert.match(pet.head, /^notes\/signoff · run-/);
      assert.equal(pet.notes, notes);
      assert.equal(pet.point, true, "Point while the notes' viewer is open");
      await w.petNotes(`${pet.notes.trim()}\nThe rounding reads right.`);
      await w.petChoose("GREEN");
      await w.petEnter("comment", "Checked the totals.");
      const done = await w.petSubmit();
      assert.equal(done.error, "", "submitted");
      const saved = request(fixture, "notes/signoff");
      assert.notEqual(saved.status, "DEFERRED", "the review is settled");
      assert.equal(saved.result?.comment, "Checked the totals.");
      await w.main();
      // Closing the viewer's window closes it alone.
      await w.closeViewer(label);
      assert.deepEqual(await w.viewers(), []);
    },
  }),

  scenario({
    id: "viewer-builtins-plugin",
    title: "the Markdown viewer renders a guide of two files and switches between them; the Artifactize Viewer draws the declared Artifacts and opens one's evals; the html plugin's page loads sandboxed from the viewer's own scheme",
    journeys: ["J-VIEWER-MARKDOWN", "J-VIEWER-ARTIFACTIZE", "J-VIEWER-PLUGIN"],
    alone: true,
    prepare: async (own) => {
      const fixture = await createViewed(own);
      return { fixture, args: ["review"], data: {} };
    },
    run: async () => {
      const w = windows();
      await w.claimReview("guide/signoff");
      await w.viewer(await w.runTool("view"));
      assert.deepEqual(await w.entriesUntil("Markdown files", (entries) => entries.length === 2, "the guide's files listed"), ["install.md", "usage.md"]);
      const install = await w.documentUntil((doc) => doc.headings.includes("Install"), "install.md rendered");
      assert.deepEqual(install.items, ["one", "two"], "a list renders as a list");
      await w.choose("Markdown files", "usage.md");
      await w.documentUntil((doc) => doc.headings[0] === "Usage", "usage.md rendered");

      await w.mode("Review");
      await w.claimReview("system/signoff");
      await w.viewer(await w.runTool("view"));
      const nodes = await w.nodesUntil((drawn) => drawn.length === 2, "the declared Artifacts drawn");
      assert.deepEqual(nodes.toSorted(), ["back custom", "front custom"], "no verdicts, only names and tags");
      const back = await w.openNode("back");
      assert.equal(back.head, "back custom");
      assert.equal(back.evals.length, 1);
      assert.match(back.evals[0], /^back\/serves runtime Title Check back Criteria Check that \{front\} is served\./);

      await w.mode("Review");
      await w.claimReview("page/signoff");
      const page = await w.runTool("view");
      assert.equal(await w.viewer(page), "page — HTML Viewer");
      const frame = await w.frameUntil((shown) => shown !== null, "the plugin's page framed");
      assert.equal(frame?.sandbox, "allow-scripts", "the page runs sandboxed, without the app's origin");
      assert.match(frame?.src ?? "", /viewer/, "served from the viewer's own scheme");
      // The page reached the bridge and says, in the strip, what it shows: its loaded page.
      const strip = await w.stripUntil((shown) => shown.items.some((item) => item.endsWith("· Checkout")), "the html page's status in the strip");
      assert.ok(!strip.items.some((item) => item.startsWith("Loading") || item.includes("did not connect")), "connected");
      assert.equal(strip.point, true, "Point in the strip for the claimed review");
      await w.main();
    },
  }),
];
