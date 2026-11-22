// Artifact Viewers, Point and the pet overlay: a review's viewer tool opens its Artifact in a
// window of its own; Point appends a place there to the review's Points; the pet overlay keeps
// the review at hand while its evaluation space is not in front, and submits it.
import assert from "node:assert/strict";
import { request } from "../fixtures/repo.ts";
import { createNotes, createViewed } from "../fixtures/viewers.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { windows } from "../views/viewers.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "viewer-point-pet",
    title: "a review's viewer tool opens the File Explorer in a window of its own; Point appends the line clicked to the review's Points; away from Review the pet overlay holds the same Points and submits the review",
    journeys: ["J-VIEWER", "J-VIEWER-FILES", "J-POINT", "J-PET", "J-HUMAN-POINTS"],
    // Its windows take the display's focus from one another.
    alone: true,
    prepare: async (own) => {
      const fixture = await createNotes(own);
      return { fixture, args: ["review"], data: {} };
    },
    run: async (app, { fixture }) => {
      const w = windows();
      await w.claimReview("notes/signoff");
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
      await w.point();
      await w.clickLine(2);
      const points = await w.pointsUntil((rows) => rows.length === 1 && /^release\.md L2:C\d+$/.test(rows[0]), "the line is in Points");
      // Away from Review the pet shows the same draft's Points.
      await w.mode("Verify");
      await w.pet();
      assert.equal(await w.title(), "Artifactize review");
      const pet = await w.petUntil((shown) => shown.choices.length > 0, "the pet offers the verdicts of the claimed review");
      assert.match(pet.head, /^notes\/signoff · run-/);
      assert.deepEqual(pet.points, points);
      assert.equal(pet.point, true, "Point while the notes' viewer is open");
      // No verdict has been chosen: the pet offers the buttons but no verdict form.
      assert.deepEqual([pet.choices, pet.radios, pet.verdict], [["GREEN", "RED"], 0, ""]);
      await w.petChoose("RED");
      await w.petChoose("GREEN");
      const chosen = await w.readPet();
      assert.deepEqual([chosen.choices, chosen.radios], [["GREEN", "RED"], 2], "one verdict choice, the form's radios");
      await w.petEnter("comment", "Checked the totals: ");
      assert.equal(await w.petInsert(), `Checked the totals: ${points[0]}`);
      const done = await w.petSubmit();
      assert.equal(done.refusal, "", "submitted");
      const saved = request(fixture, "notes/signoff");
      assert.notEqual(saved.status, "DEFERRED", "the review is settled");
      assert.equal(saved.result?.comment, `Checked the totals: ${points[0]}`);
      await w.mode("Review");
      await app.review.until((shown) => shown.completed.includes(`Checked the totals: ${points[0]}`) && !shown.sections.includes("Points"), "the submitted review shows the form without Points");
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
      // The page reached the bridge and, once its inner page loaded, reported the page and its
      // title: the strip's one status line says so, in place of Loading, a page that did not
      // connect or a page that did not load.
      const strip = await w.stripUntil((shown) => shown.page === "checkout/index.html · Checkout", "the HTML Viewer reports the loaded Checkout document");
      assert.ok(!strip.items.some((item) => item.startsWith("Loading") || item.includes("did not connect") || item.includes("did not load")), "no connection or load failure in the strip");
      assert.equal(strip.point, true, "the claimed review can Point in the plugin");
      await w.main();
    },
  }),
];
