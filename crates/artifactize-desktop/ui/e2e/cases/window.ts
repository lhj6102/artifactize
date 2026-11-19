// The window: how it opens in each launch mode, its one screen, the palette, Help, opening a
// request by ID, a newer release, and quitting.
import assert from "node:assert/strict";
import { addRepository, addWorktrees, createRelease, request } from "../fixtures/repo.ts";
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
      await addRepository(fixture, "other");
      return { fixture, data: { run, worktree } };
    },
    run: async (app, { data: { run, worktree } }) => {
      await app.pane.branch("other  master");
      await app.pane.branch("repo  scope-one");
      const names = (await app.pane.branches()).map((branch) => branch.name).sort();
      assert.deepEqual(names, ["other  master", "repo  master", "repo  scope-one"]);
      assert.equal(await app.windowTitle(), "Artifactize");
      assert.equal(await app.graph.title(), "Nothing chosen");
      assert.equal(await app.graph.empty(), "Choose a branch or a waiting review in the Pane.");
      assert.deepEqual(await app.pane.layout(), { parts: ["Search branches and reviews", "Branches", "Waiting"], query: "sort:recent", divider: true, scope: false });
      // Folded, a branch reads as project and branch over its newest Run's status.
      const folded = await app.pane.branch("repo  scope-one");
      assert.equal(folded.label, "repo / scope-one");
      assert.equal(folded.expanded, false);
      assert.match(folded.latest, /^Waiting for sign-off \d+[smh]/);
      // A click on its name expands it to its timeline, its newest Run shown in the main area.
      const expanded = await app.pane.selectBranch("repo  master");
      assert.equal(expanded.latest, "", "expanded, the timeline replaces the newest Run's status");
      // Its one Run, in progress, is its newest entry and the one shown.
      assert.deepEqual(expanded.entries.map((entry) => [entry.id, entry.shown]), [[run, true]]);
      assert.match(expanded.entries[0].text, /^Waiting for sign-off \d+[smh]/);
      await app.graph.titled(run);
      assert.match(await app.graph.headline(), /^WAITING_HUMAN: release\/signoff /);
      await app.graph.node("a:release", (node) => node.label === "release: in progress, 0 of 1 evals GREEN", "release waits for its sign-off");
      await app.graph.node("a:docs", (node) => node.label === "docs: GREEN, 1 of 1 evals GREEN", "docs passed");
      // Another branch's timeline entry shows its own Run.
      await app.pane.choose("repo  scope-one", worktree);
      await app.graph.titled(worktree);
      // Folded again, the branch reads its newest Run's status again.
      assert.match((await app.pane.toggle("repo  master")).latest, /^Waiting for sign-off/);
    },
  }),
];
