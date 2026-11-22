// The window: how it opens, its one screen, the command palette and Help, opening a request by
// ID, a newer release, refreshing and quitting.
import assert from "node:assert/strict";
import { installChatProvider } from "../fixtures/chat.ts";
import { standInBrowser, browserHandoffs } from "../fixtures/browser.ts";
import { addRepository, addWorktree, addWorktrees, createEmpty, createRelease, declareRelease, environment, newFixture, orderRuns, removeWorktree, request, requests, verifyWorktree } from "../fixtures/repo.ts";
import { shortcutLabel } from "../platform.ts";
import { nativePath, readText, writeText, serveLoopbackHttp } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "launch-monitor",
    title: "opens on the pane of every live branch beside the main area with nothing chosen; a branch folds to its newest Run and expands to its timeline, and a Run fills the main area with its graph",
    journeys: ["J-LAUNCH", "J-PANE", "J-BRANCHES", "J-BRANCH-RUNS", "J-MAIN-RUN"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      const run = request(fixture).runId;
      const [worktree] = await addWorktrees(fixture, ["scope-one"]);
      // A second, newer Run of scope-one.
      const newer = await verifyWorktree(fixture, "scope-one");
      orderRuns(fixture, [worktree, newer]);
      await addRepository(fixture, "other");
      return { fixture, data: { run, worktree, newer } };
    },
    run: async (app, { fixture, data: { run, worktree, newer } }) => {
      await app.pane.branch("other  master");
      await app.pane.branch("repo  scope-one");
      const names = (await app.pane.branches()).map((branch) => branch.name).sort();
      assert.deepEqual(names, ["other  master", "repo  master", "repo  scope-one"]);
      assert.equal(await app.windowTitle(), "Artifactize");
      assert.equal(await app.graph.title(), "Nothing chosen");
      assert.equal(await app.graph.empty(), "Choose a branch in the Pane.");
      assert.deepEqual(await app.pane.layout(), { parts: ["Search the branches", "Branches"], query: "sort:recent", divider: true, scope: false });
      assert.equal(await app.pane.stacked(), true, "search and branches in one column");
      // Folded, a branch reads as project and branch over its newest Run's status.
      const folded = await app.pane.branch("repo  scope-one");
      assert.equal(folded.label, "repo / scope-one");
      assert.equal(folded.expanded, false);
      assert.match(folded.latest, /^Waiting for sign-off \d+[smh]/);
      // A click on its name expands it to its timeline, its newest Run shown in the main area.
      const expanded = await app.pane.selectBranch("repo  master");
      assert.equal(expanded.latest, "", "expanded, the timeline replaces the newest Run's status");
      assert.deepEqual(expanded.entries.map((entry) => [entry.id, entry.shown]), [[run, true]], "its Run in progress is its newest entry");
      assert.match(expanded.entries[0].text, /^Waiting for sign-off \d+[smh]/);
      await app.graph.titled(run);
      // The title shows at once; the headline once the Run, drawn here, has loaded.
      await app.graph.node("a:release", (node) => node.label === "release: in progress, 0 of 1 evals GREEN", "release waits for its sign-off");
      await app.graph.node("a:docs", (node) => node.label === "docs: GREEN, 1 of 1 evals GREEN", "docs passed");
      // ship's eval reads release: an edge leads from release to ship.
      await app.graph.edge("a:release", "a:ship");
      assert.match(await app.graph.headline(), /^WAITING_HUMAN: release\/signoff /);
      // Another branch's timeline lists its Runs newest first; a click on the older shows it.
      if (!(await app.pane.branch("repo  scope-one")).expanded) await app.pane.toggle("repo  scope-one");
      const timeline = await app.pane.branch("repo  scope-one", (branch) => branch.entries.length === 2, "scope-one's two Runs listed");
      assert.deepEqual(timeline.entries.map((entry) => entry.id), [newer, worktree], "newest first");
      await app.pane.choose("repo  scope-one", worktree);
      await app.graph.titled(worktree);
      // A worktree added by another process is listed within seconds, and goes when removed.
      addWorktree(fixture, "scope-live");
      await app.pane.branch("repo  scope-live");
      removeWorktree(fixture, "scope-live");
      await app.until(async () => !(await app.pane.branches()).some((branch) => branch.name === "repo  scope-live"), "the removed worktree's branch gone");
      // Folded again, the branch reads its newest Run's status again.
      assert.match((await app.pane.toggle("repo  master")).latest, /^Waiting for sign-off/);
    },
  }),

  scenario({
    id: "palette-help",
    title: "the brand button opens the palette listing what can run now with its keys, typing filters it and a click runs a command; its Keyboard shortcuts opens Help with the keys and, on a graph, the marks",
    journeys: ["J-PALETTE", "J-HELP"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      await app.graph.node("a:release");
      const palette = await app.openPalette();
      const commands = palette.commands.map((command) => [command.label, command.keys]);
      for (const expected of [["Keyboard shortcuts", "?"], ["Show Settings", shortcutLabel("Ctrl+,")], ["Show or hide the Pane", shortcutLabel("Ctrl+B")],
        ["Search the branches", "/"], ["Open a request by ID", shortcutLabel("Ctrl+O")], ["Refresh", shortcutLabel("Ctrl+R")], ["Quit", "q"], ["Find", shortcutLabel("Ctrl+F")]]) {
        assert.ok(commands.some(([label, keys]) => label === expected[0] && keys.includes(expected[1])), `the palette lists ${expected[0]} with ${expected[1]}: ${JSON.stringify(commands)}`);
      }
      // The app's commands come before the graph's.
      assert.ok(commands.findIndex(([label]) => label === "Quit") < commands.findIndex(([label]) => label === "Find"));
      await app.type("ow Sett");
      assert.deepEqual((await app.palette())?.commands.map((command) => command.label), ["Show Settings"], "typing filters the commands");
      await app.press(["Escape"], async () => (await app.palette()) === null, "the palette closed");
      // A click on a command runs it.
      await app.openPalette();
      await app.runCommand("Show Settings");
      await app.settings.until((shown) => shown.tabs.some((tab) => tab.name === "Providers" && tab.selected), "Settings opened by the palette");
      await app.settings.close("Close");
      // Keyboard shortcuts opens Help: the keys of what has focus and, with the graph focused, its marks.
      await app.graph.focus();
      await app.command("Keyboard shortcuts");
      const help = (await app.help())!;
      assert.ok(help.keys.some((key) => key.action === "Command palette" && key.keys === shortcutLabel("Ctrl+K")), `Help lists the palette's key: ${JSON.stringify(help.keys)}`);
      assert.deepEqual(help.marks.map((mark) => mark.word), ["GREEN", "in progress", "waiting", "RED", "ERROR", "blocked by upstream"], "a Run's marks, each with its word");
      assert.ok(help.marks.every((mark) => mark.icon !== ""), "each mark beside its word");
      await app.closeHelp("Close");
      await app.command("Keyboard shortcuts");
      await app.closeHelp("outside");
    },
  }),

  scenario({
    id: "open-review-by-id",
    title: "started with review ID it opens that waiting review in the main area, named once in its header; the palette's Open a request by ID opens another request read-only, and choosing a branch leaves the review",
    journeys: ["J-LAUNCH", "J-REVIEW-REF", "J-OPEN-ID", "J-HUMAN-LAYOUT"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      const first = request(fixture).id;
      await addRepository(fixture, "other");
      const docs = requests(fixture).find((saved) => saved.evalId === "docs/lint")!.id;
      return { fixture, args: ["review", first], data: { first, docs } };
    },
    run: async (app, { data: { first, docs } }) => {
      await app.review.titleContains("release/signoff");
      assert.match(await app.review.debugState(), new RegExp(`^Request ${first} · DEFERRED · unclaimed`), "the named review, not the newest one");
      await app.review.until((shown) => shown.stage === "CLAIM" && shown.tools.length > 0, "the review is actionable: it can be claimed");
      await app.review.until((shown) => ["Instruction", "Tools", "Tool log", "Result"].every((section) => shown.sections.includes(section)), "the review shows the instruction, Tools, Tool log and Result");
      // Another request by its ID opens read-only.
      await app.openPalette();
      await app.runCommand("Open a request by ID");
      await app.openById(docs);
      await app.review.titleContains("docs/lint");
      assert.equal((await app.graph.recordDebug()).Request, docs, "the request of the ID entered, not the other repository's");
      const record = await app.graph.record();
      assert.match(record.status, /^GREEN/, "its saved result");
      assert.equal(record.instruction, "Check docs.", "for its instruction");
      // Choosing a branch leaves what the main area showed.
      await app.mode("Verify");
      const branch = await app.pane.selectBranch("repo  master");
      await app.graph.titled(branch.entries.find((entry) => entry.shown)!.id);
    },
  }),

  scenario({
    id: "quit-with-claims",
    title: "Quit and the window's close request both ask while a claim is held; Stay keeps it, Release and quit releases it and exits",
    journeys: ["J-QUIT"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["review"], data: { id: request(fixture).id } };
    },
    run: async (app, { fixture, data: { id }, port }) => {
      await app.review.titleContains("release/signoff");
      await app.review.claim();
      await app.command("Quit");
      const asked = await app.dialogTitled("Leave the review?");
      assert.match(asked.text, new RegExp(`This window holds 1 review claim: .*${id}`));
      await app.dialogButton("Stay");
      await app.dialogClosed();
      assert.equal(request(fixture).claim?.reviewer, "alice", "Stay keeps the claim");
      await app.review.until((shown) => shown.stage === "REVIEW (yours)", "the window and its review stay");
      await app.closeWindow();
      await app.dialogTitled("Leave the review?");
      await app.exitsBy(() => app.dialogButton("Release and quit"), port);
      assert.equal(request(fixture).claim, null, "Release and quit released the claim");
    },
  }),

  scenario({
    id: "quit-closing-the-window",
    title: "closing the window with no claim held quits at once, asking nothing",
    journeys: ["J-QUIT"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app, { port }) => {
      await app.graph.node("a:release");
      await app.exitsBy(() => app.closeWindow(), port);
    },
  }),

  scenario({
    id: "update-notice",
    title: "a newer stable release shows as `{version} available` in Settings; its dialog names both versions and links the release page, which opens in the browser",
    journeys: ["J-UPDATE"],
    prepare: async (own) => {
      const fixture = createEmpty(own);
      const visits = nativePath(own.root, "site-visits");
      writeText(visits, "");
      const site = await serveLoopbackHttp((incoming, response) => {
        incoming.resume();
        writeText(visits, `${readText(visits)}${incoming.url}\n`);
        const lookup = incoming.url === "/api/releases/latest";
        response.writeHead(200, { "content-type": lookup ? "application/json" : "text/html" });
        response.end(lookup ? JSON.stringify({ tag_name: "v9.9.9" }) : "<title>Release notes</title>");
      });
      own.defer(() => site.close());
      const env = standInBrowser(fixture, { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: site.url, ARTIFACTIZE_NO_UPDATE_CHECK: undefined });
      return { fixture, env, data: { site: site.url, visits } };
    },
    run: async (app, { fixture, data: { site, visits } }) => {
      await app.settings.open();
      await app.settings.openUpdate("9.9.9");
      await app.dialogTitled("Update artifactize");
      const dialog = (await app.releaseDialog())!;
      assert.match(dialog.versions, / → 9\.9\.9$/, "the running and the new version");
      assert.equal(dialog.link, `${site}/releases/latest`, "how to update: the release page");
      await app.followReleaseLink();
      await app.eventually(async () => browserHandoffs(fixture).includes(dialog.link), "the release page opened in the browser");
      await app.eventually(async () => readText(visits).split("\n").includes("/releases/latest"), "the browser fetched the release page");
      await app.closeRelease();
      await app.settings.close("Close");
    },
  }),

  scenario({
    id: "refresh",
    title: "the palette's Refresh scans the shown Current again: a definition changed on disk shows only after it, on the same view",
    journeys: ["J-REFRESH"],
    prepare: async (own) => {
      const fixture = newFixture(own);
      declareRelease(fixture);
      await installChatProvider(own, fixture);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { definition: nativePath(fixture.repo, ".artifactize", "docs.artf") } };
    },
    run: async (app, { data: { definition } }) => {
      await app.chat.createFromDialog();
      await app.chat.graph();
      await app.graph.node("a:docs");
      await app.graph.open("a:docs", "Artifact docs");
      await app.detail.openEval("docs/lint");
      // Its first scan over, nothing reads the Current's worktree again here but a Refresh.
      await app.graph.until(async () => /^Scanned at /.test(await app.graph.scan()), "the Current scanned");
      assert.equal((await app.detail.summary()).instruction, "Check docs.");
      // What only a Refresh shows is a change in the Current's worktree. A change of the state
      // or of the branches reaches the open app without a reload too (J-LIVE, J-BRANCHES), so a
      // Run or a branch listed after a Refresh would show nothing of the Refresh itself.
      writeText(definition, readText(definition).replace('payload.instruction = "Check docs."', 'payload.instruction = "Check the docs again."'));
      const title = await app.graph.title();
      assert.equal((await app.detail.summary()).instruction, "Check docs.", "the change on disk does not show before the Refresh");
      await app.command("Refresh");
      await app.detail.summaryUntil((summary) => summary.instruction === "Check the docs again.", "the Refresh read the changed definition");
      assert.equal(await app.graph.title(), title, "on the same view");
    },
  }),
];
