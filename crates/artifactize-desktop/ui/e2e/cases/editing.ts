// Editing a branch's definitions on its Current: the Definition and Source, parts, new, renamed
// and deleted Artifacts, places by hand and comments, each written into the files the CLI reads.
import assert from "node:assert/strict";
import { CORE_NOTE, createEditing, definitionText, rewriteDefinition } from "../fixtures/repo.ts";
import { scenario, type Scenario } from "../scenario.ts";

/** Whether the definition file text `text` places its Artifact by hand. */
const placed = (text: string | null) => /^at = \[/m.test(text ?? "");

export const scenarios: Scenario[] = [
  scenario({
    id: "definition-edit",
    title: "on a Current an Artifact's tab has its Definition: a Path written by Enter, a missing target said beside it and a refused one in red, a typed Path kept as a draft when the Detail closes, and a Source that Save writes and Revert puts back",
    journeys: ["J-DEFINITION", "J-DEFINITION-EDIT", "J-DEFINITION-DRAFT"],
    prepare: async (own) => {
      const fixture = createEditing(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.open("a:core", "Artifact core");
      let shown = await app.detail.definitionUntil("core", (each) => each.file === ".artifactize/core.artf", "core's Definition names its file");
      assert.equal(shown.path, "core/");
      assert.equal(shown.source, null, "the Source folded until unfolded");
      // A Path is written by Enter; a missing target is said beside it.
      await app.detail.writePath("core", "gone/", (each) => each.missing === "gone/ does not exist.", "the missing target said beside the Path");
      await app.eventually(() => definitionText(fixture, "core")?.includes('path = "gone/"') === true, "the file holds the new Path");
      // A Path the project would not accept writes nothing and says why.
      const kept = definitionText(fixture, "core");
      await app.detail.writePath("core", "../outside/", (each) => each.refusals.length > 0, "the Path refused under it");
      assert.equal(definitionText(fixture, "core"), kept, "a refused Path writes nothing");
      await app.detail.discardPath("core", (each) => each.path === "gone/" && each.refusals.length === 0, "Esc put the Path back, its refusal gone");
      // A Path being typed is kept when the Detail closes, and not written.
      await app.detail.typePath("core", "kept-draft/");
      await app.detail.close();
      await app.detail.closed();
      await app.graph.open("a:core", "Artifact core");
      shown = await app.detail.definitionUntil("core", (each) => each.path === "kept-draft/", "the draft shown again");
      assert.equal(definitionText(fixture, "core"), kept, "the draft was not written");
      await app.detail.writePath("core", "core/", (each) => each.path === "core/" && each.missing === "" && each.refusals.length === 0, "core/ shown again without the missing target");
      await app.eventually(() => definitionText(fixture, "core")?.includes('path = "core/"') === true, "the Path written back");
      // Typing in the Source offers Save, which writes the file; Revert puts back its text.
      const text = definitionText(fixture, "core")!;
      await app.detail.editSource("core", `${text}# reviewed\n`);
      assert.equal(definitionText(fixture, "core"), text, "nothing written before Save");
      await app.detail.sourceAction("Save");
      await app.eventually(() => definitionText(fixture, "core")?.endsWith("# reviewed\n") === true, "Save wrote the Source");
      let saved = definitionText(fixture, "core")!;
      await app.detail.definitionUntil("core", (each) => each.source === saved && !each.saving, "the saved Source shown, Save gone");
      // A part declared in the Source reaches the Definition and the graph once saved.
      await app.detail.editSource("core", `${saved}parts = ["kit"]\n`);
      await app.detail.sourceAction("Save");
      await app.detail.definitionUntil("core", (each) => each.parts.includes("kit") && !each.saving, "the Definition lists kit");
      await app.graph.node("a:core/a:kit", () => true, "the graph draws kit in core's box");
      await app.detail.removePart("core", "kit");
      await app.detail.definitionUntil("core", (each) => each.parts.length === 0 && !each.saving, "kit left core again");
      saved = definitionText(fixture, "core")!;
      await app.detail.definitionUntil("core", (each) => each.source === saved, "the Source shows the file as written");
      await app.detail.editSource("core", `${saved}parts = ["nobody"]\n`);
      await app.detail.sourceAction("Save");
      await app.detail.definitionUntil("core", (each) => each.refusals.some((refusal) => /line \d/.test(refusal)), "the refusal names its line");
      assert.equal(definitionText(fixture, "core"), saved, "an invalid Source writes nothing");
      await app.detail.sourceAction("Revert");
      await app.detail.definitionUntil("core", (each) => each.source === saved && !each.saving, "Revert put back the file's text");
    },
  }),

  scenario({
    id: "definition-conflict",
    title: "graph edits preserve unsaved Source, and edits of files changed by another writer are refused and show what is on disk",
    journeys: ["J-DEFINITION-CONFLICT"],
    prepare: async (own) => {
      const fixture = createEditing(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.open("a:core", "Artifact core");
      await app.detail.definitionUntil("core", (each) => each.path === "core/", "core's Definition");
      const before = definitionText(fixture, "core")!;
      const unsaved = `${before}# Unsaved review comment\n`;
      await app.detail.editSource("core", unsaved);
      await app.graph.drag("a:core", 0, 60);
      await app.graph.expandNotice("Has unsaved text", ".artifactize/core.artf has unsaved text in its Source; nothing written.");
      await app.detail.definitionUntil("core", (shown) => shown.source === unsaved, "the typed Source text stayed in the editor");
      assert.equal(definitionText(fixture, "core"), before, "the graph edit left the file untouched");
      await app.detail.sourceAction("Revert");
      await app.detail.definitionUntil("core", (shown) => shown.source === before, "Revert put back the saved Source");
      const theirs = definitionText(fixture, "core")!.replace("basis = true\n", "basis = true\nparts = [\"kit\"]\n");
      rewriteDefinition(fixture, "core", theirs);
      await app.detail.writePath("core", "moved/", (each) => each.refusals.includes(".artifactize/core.artf changed on disk; nothing written."), "the edit refused, naming the file");
      assert.equal(definitionText(fixture, "core"), theirs, "the file as the other writer left it");
      await app.graph.node("a:core/a:kit", () => true, "the Current scanned again: kit drawn in core's box");
    },
  }),

  scenario({
    id: "parts-edit",
    title: "+ Add under Parts adds a suggested Artifact to the file's parts and the graph draws it inside the whole's box; ✕ removes it and the graph draws it on its own again",
    journeys: ["J-PARTS-EDIT"],
    prepare: async (own) => {
      const fixture = createEditing(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      // docs, on its own at the top level, becomes core's part and then leaves it.
      await app.graph.node("a:docs");
      await app.graph.open("a:core", "Artifact core");
      await app.detail.addPart("core", "docs");
      await app.eventually(() => /^parts = \["docs"\]/m.test(definitionText(fixture, "core") ?? ""), "core's file names docs as a part");
      await app.graph.node("a:core/a:docs", () => true, "docs drawn inside core's box");
      await app.graph.gone("a:docs");
      await app.detail.removePart("core", "docs");
      await app.eventually(() => !/^parts = /m.test(definitionText(fixture, "core") ?? ""), "docs left core's parts");
      await app.graph.gone("a:core/a:docs");
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.node("a:docs", () => true, "docs drawn on its own again");
    },
  }),

  scenario({
    id: "artifacts-new-rename-delete",
    title: "New Artifact writes a definition and opens the new node's Detail, refusing a name already used; Rename renames the file and every reference to it; Delete removes a file nothing names and refuses one others name",
    journeys: ["J-NODE-NEW", "J-NODE-RENAME", "J-NODE-DELETE"],
    prepare: async (own) => {
      const fixture = createEditing(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.node("a:core");
      await app.graph.tool("New Artifact");
      assert.equal((await app.detail.dialog())?.title, "New Artifact");
      const refused = await app.detail.submitDialog(["Core", "core/"], "Create", true);
      assert.match(refused?.refusal ?? "", /already exists/i, "a name already used, ignoring case, is refused");
      assert.equal(await app.detail.submitDialog(["extra", "extra/"], "Create"), null, "Create closed the dialog");
      await app.eventually(() => definitionText(fixture, "extra")?.includes('path = "extra/"') === true, "extra's file written");
      await app.graph.node("a:extra");
      await app.detail.until((shown) => shown.title === "Artifact extra", "the new Artifact's Detail opened");
      // Rename: the file and every reference follow.
      await app.graph.open("a:core", "Artifact core");
      await app.detail.definitionAction("Rename");
      assert.equal((await app.detail.dialog())?.title, "Rename core");
      assert.equal(await app.detail.submitDialog(["base"], "Rename"), null, "Rename closed the dialog");
      await app.eventually(() => definitionText(fixture, "base") !== null && definitionText(fixture, "core") === null, "core's file renamed");
      assert.ok(definitionText(fixture, "docs")?.includes("{base}"), "docs references base now");
      await app.detail.until((shown) => shown.title === "Artifact base", "the Detail names base");
      await app.graph.node("a:base");
      // kit is named by app and bundle: its deletion is refused, saying who names it.
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.open("a:app/a:kit", "Artifact kit");
      await app.detail.definitionAction("Delete");
      const blocked = await app.detail.submitDialog([], "Delete", true);
      assert.match(blocked?.refusal ?? "", /app/, `the refusal names who holds kit: ${blocked?.refusal}`);
      assert.notEqual(definitionText(fixture, "kit"), null, "nothing written");
      await app.detail.cancelDialog();
      // extra, which nothing names, is deleted.
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.open("a:extra", "Artifact extra");
      await app.detail.definitionAction("Delete");
      assert.match((await app.detail.dialog())?.text ?? "", /\.artifactize\/extra\.artf is removed/);
      assert.equal(await app.detail.submitDialog([], "Delete"), null, "Delete closed the dialog");
      await app.eventually(() => definitionText(fixture, "extra") === null, "extra's file removed");
      await app.detail.closed();
      await app.graph.gone("a:extra");
    },
  }),

  scenario({
    id: "layout-by-hand",
    // Its app is started again: the phases run one after the other, alone.
    alone: true,
    title: "a node dragged on a Current is placed where it is released, every place of its level written into the files and kept when the app starts again, Reset layout lays it out automatically again, a node dropped on a whole becomes its part, and a part dragged onto another whole moves there",
    journeys: ["J-NODE-MOVE", "J-LAYOUT-RESET", "J-PARTS-DRAG"],
    prepare: async (own) => {
      const fixture = createEditing(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.node("a:core");
      assert.equal(await app.graph.hasTool("Reset layout"), false, "laid out automatically");
      // Where core is beside docs, at the zoom shown: whatever the camera does.
      const offset = async () => {
        const [core, docs, zoom] = [(await app.graph.box("a:core"))!, (await app.graph.box("a:docs"))!, await app.graph.zoom()];
        return { x: (core.x - docs.x) / (zoom / 100), y: (core.y - docs.y) / (zoom / 100) };
      };
      const automatic = await offset();
      const before = (await app.graph.box("a:core"))!;
      await app.graph.drag("a:core", 0, 60);
      await app.eventually(() => ["app", "bundle", "core", "docs"].every((name) => placed(definitionText(fixture, name))), "every top-level place written");
      // The graph draws core at its saved place: where it was released.
      await app.graph.until(async () => {
        const after = await app.graph.box("a:core");
        return !!after && Math.abs(after.y - before.y - 60) <= 12 && Math.abs(after.x - before.x) <= 12;
      }, "core drawn where it was released");
      // For the app started again: where core was laid out automatically, and where it is now.
      const placedAt = await offset();
      // The next graph launch must switch this saved session back from Conversation.
      await app.chat.show("Conversation");
      return { automatic, placed: placedAt };
    },
    // A later app draws core where it was placed, beside the others; Reset layout then lays the
    // level out automatically again, and a node dropped on a whole becomes its part.
    restarted: async (app, { fixture }, carried) => {
      const { automatic, placed: placedAt } = carried as Record<"automatic" | "placed", { x: number; y: number }>;
      await app.chat.viewShown("Graph");
      await app.graph.node("a:core");
      const offset = async () => {
        const [core, docs, zoom] = [(await app.graph.box("a:core"))!, (await app.graph.box("a:docs"))!, await app.graph.zoom()];
        return { x: (core.x - docs.x) / (zoom / 100), y: (core.y - docs.y) / (zoom / 100) };
      };
      const restarted = await offset();
      assert.ok(Math.abs(restarted.x - placedAt.x) <= 8 && Math.abs(restarted.y - placedAt.y) <= 8, `core placed as it was dropped: ${JSON.stringify({ placedAt, restarted })}`);
      await app.graph.until(() => app.graph.hasTool("Reset layout"), "Reset layout offered for the top level");
      await app.graph.tool("Reset layout");
      await app.eventually(() => ["app", "bundle", "core", "docs"].every((name) => !placed(definitionText(fixture, name))), "every place left the files");
      await app.graph.until(async () => {
        const reset = await offset();
        return Math.abs(reset.x - automatic.x) <= 8 && Math.abs(reset.y - automatic.y) <= 8;
      }, "core drawn beside docs as the automatic layout had it");
      // docs dropped on app's title becomes app's part, drawn under the part it would cover there.
      await app.graph.dragOnto("a:docs", "a:app");
      await app.eventually(() => /^parts = \[.*"docs".*\]/m.test(definitionText(fixture, "app") ?? ""), "docs added to app's parts");
      await app.eventually(() => /^docs = \[/m.test(definitionText(fixture, "app") ?? ""), "app's file places docs in its box");
      await app.graph.until(async () => {
        const [whole, part, kit] = [await app.graph.box("a:app"), await app.graph.box("a:app/a:docs"), await app.graph.box("a:app/a:kit")];
        return !!whole && !!part && !!kit && part.y >= kit.y + kit.height - 1
          && part.x >= whole.x && part.y >= whole.y && part.x + part.width <= whole.x + whole.width && part.y + part.height <= whole.y + whole.height;
      }, "docs drawn inside app's box, under kit");
      await app.graph.gone("a:docs");
      // Dragged out of app's box onto bundle, docs moves: bundle's part, placed in its box, and
      // gone from app's parts and from the places app's file holds.
      await app.graph.dragOnto("a:app/a:docs", "a:bundle");
      await app.eventually(() => /^parts = \[.*"docs".*\]/m.test(definitionText(fixture, "bundle") ?? "") && /^docs = \[/m.test(definitionText(fixture, "bundle") ?? ""),
        "docs added to bundle's parts, its place in bundle's file");
      await app.eventually(() => !/"docs"/.test(definitionText(fixture, "app")?.match(/^parts = .*$/m)?.[0] ?? "") && !/^docs = \[/m.test(definitionText(fixture, "app") ?? ""),
        "docs left app's parts and app's file no longer places it");
      await app.graph.node("a:bundle/a:docs");
      await app.graph.gone("a:app/a:docs");
    },
  }),

  scenario({
    id: "comments",
    title: "a comment is drawn under its node; the Definition's Comment opens a field under the node whose Enter writes it, a click on a comment opens it to change it, and a comment dragged onto another Artifact moves to its file",
    journeys: ["J-COMMENT", "J-COMMENT-EDIT", "J-COMMENT-MOVE"],
    prepare: async (own) => {
      const fixture = createEditing(own, { comments: true });
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.node("a:core", (node) => node.comment === CORE_NOTE, "core's comment drawn under it");
      // The first scan over, nothing redraws the Detail while a comment is typed.
      await app.graph.until(async () => /^Scanned at /.test(await app.graph.scan()), "the Current scanned");
      // The Definition's Comment opens the field under docs; Enter writes it.
      await app.graph.open("a:docs", "Artifact docs");
      await app.detail.definitionAction("Comment");
      const note = "Read the core first\nthen the docs";
      // The comment as a file holds it: a multi-line string of its TOML.
      const saved = `\n[annotations.comment]\ntext = """\n${note}"""\n`;
      await app.detail.writeComment("docs", note);
      await app.eventually(() => definitionText(fixture, "docs")?.endsWith(saved) === true, "docs's file holds the comment as it was entered");
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.node("a:docs", (node) => node.comment === note, "the comment drawn under docs");
      // A click on core's comment opens it; an empty field removes it.
      await app.detail.openComment("a:core", "core");
      await app.detail.writeComment("core", "");
      await app.eventually(() => !/annotations/.test(definitionText(fixture, "core") ?? ""), "the emptied comment left core's file");
      await app.graph.node("a:core", (node) => node.comment === "", "nothing drawn under core");
      // docs's comment dragged onto core, which has none now, moves there.
      await app.graph.dragComment("a:docs", "a:core");
      await app.eventually(() => definitionText(fixture, "core")?.endsWith(saved) === true && !/annotations/.test(definitionText(fixture, "docs") ?? ""),
        "the comment moved, as it was, from docs's file to core's");
      await app.graph.node("a:core", (node) => node.comment === note, "drawn under core");
      await app.graph.node("a:docs", (node) => node.comment === "", "and no longer under docs");
    },
  }),
];
