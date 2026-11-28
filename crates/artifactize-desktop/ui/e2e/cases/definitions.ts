// A branch's definitions read on its Current: an Artifact's Definition (where it is bound and by
// what macro, its Path, its Parts and its Tools) and its Source, as written and, for an Artifact
// a macro made, as the call expands.
import assert from "node:assert/strict";
import { createDefinitions } from "../fixtures/repo.ts";
import { nativePath, readText } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

/** The line of `text` that starts with `start`, counted from 1. */
const lineOf = (text: string, start: string) => text.split("\n").findIndex((line) => line.startsWith(start)) + 1;

export const scenarios: Scenario[] = [
  scenario({
    id: "definition-read",
    title: "on a Current an Artifact's tab has its Definition, nothing in it a field: where it is bound, its macro for one a macro made, its Path, Parts that open their own Detail and its Tools; Source, folded until unfolded, shows its code numbered by its lines in its file, and for a macro's Artifact switches between Authored, the call, and Expanded, the declaration it makes",
    journeys: ["J-DEFINITION", "J-DEFINITION-SOURCE"],
    prepare: async (own) => {
      const fixture = createDefinitions(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { file: readText(nativePath(fixture.repo, ".artifactize", "project.artf")) } };
    },
    run: async (app, { data: { file } }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      // Written out: where it is bound, its Path, its part and its Tools.
      await app.graph.open("a:app", "Artifact app");
      let shown = await app.detail.definitionUntil("app", (each) => each.facts.Bound !== undefined, "app's Definition shown");
      assert.equal(shown.facts.Bound, `.artifactize/project.artf:${lineOf(file, "artf app ")}`, "where it is bound, its file and line");
      assert.equal(shown.facts.Macro, undefined, "an Artifact written out names no macro");
      assert.equal(shown.facts.Path, "app/");
      assert.equal(shown.missing, "", "its target exists");
      assert.deepEqual(shown.parts, ["kit"]);
      assert.equal(shown.facts.Tools, "files");
      assert.equal(shown.fields, 0, "nothing in the Definition is a field");
      assert.equal(shown.source, null, "the Source folded until unfolded");
      await app.detail.toggleSection("Source");
      shown = await app.detail.definitionUntil("app", (each) => each.source !== null, "app's Source shown");
      assert.deepEqual(shown.views, [], "an Artifact written out has only its code");
      const lines = shown.source!.split("\n");
      const head = lines.findIndex((line) => line.startsWith("artf app : files = {"));
      assert.ok(head >= 0, `its code as written: ${shown.source}`);
      assert.equal(shown.numbers[head], lineOf(file, "artf app "), "each line numbered as in its file");
      assert.equal(shown.numbers.length, lines.length, "every line numbered");
      assert.equal(shown.fields, 0, "the Source is no field");
      // A part opens its own Detail.
      await app.detail.openPart("app", "kit");
      await app.detail.definitionUntil("kit", (each) => each.facts.Path === "app/kit/", "kit's Definition shown");

      // Made by a macro: its macro named, its Source on Authored first, then Expanded.
      await app.graph.open("a:core", "Artifact core");
      shown = await app.detail.definitionUntil("core", (each) => each.facts.Macro === "crate!", "core names the macro that made it");
      assert.equal(shown.facts.Bound, `.artifactize/project.artf:${lineOf(file, "artf core ")}`);
      assert.equal(shown.facts.Path, "core/");
      assert.equal(shown.facts.Tools, "files");
      if (!(await app.detail.read())!.sections.some((section) => section.name === "Source" && section.open)) await app.detail.toggleSection("Source");
      shown = await app.detail.definitionUntil("core", (each) => each.source !== null && each.view !== "", "core's Source shown");
      assert.deepEqual(shown.views, ["Authored", "Expanded"]);
      assert.equal(shown.view, "Authored", "it opens on Authored");
      assert.equal(shown.source, 'artf core = crate!(dir = "core")', "the call as written");
      assert.deepEqual(shown.numbers, [lineOf(file, "artf core ")]);
      await app.detail.sourceView("core", "Expanded");
      shown = await app.detail.definitionUntil("core", (each) => each.view === "Expanded" && each.source !== 'artf core = crate!(dir = "core")', "core's expansion shown");
      assert.match(shown.source!, /^artf core : files = \{/m, "the declaration the call makes");
      assert.match(shown.source!, /path = "core\/"/, "its parameter filled in");
      assert.match(shown.source!, /eval check : runtime\(/, "with the eval the macro gives it");
      assert.doesNotMatch(shown.source!, /@\{param /, "no parameter left unfilled");
      await app.detail.sourceView("core", "Authored");
    },
  }),
];
