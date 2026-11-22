// The left pane: hiding and resizing it, starring a branch, searching, and the reviews waiting
// for a person as other processes change them.
import assert from "node:assert/strict";
import { addRepository, addWorktrees, artifactize, createProject, createRelease, declareRelease, orderRuns, request, requests, savedRuns, verify, verifyWaiting } from "../fixtures/repo.ts";
import { nativePath, removeTree, type NativePath } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "pane-toggle-resize",
    title: "the Pane switch hides the pane and its divider, the main area taking the window's width with its view, and shows it again as it was; dragging the divider and the Detail's edge resizes them, kept when shown again",
    journeys: ["J-PANE-TOGGLE", "J-RESIZE"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      await addRepository(fixture, "other");
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      await app.graph.node("a:release");
      const run = await app.graph.title();
      // The pane as the person left it: dragged wider, a search of their own, a branch expanded.
      const width = await app.pane.dragDivider(80);
      await app.pane.searchFor("sort:name");
      await app.pane.toggle("other  master");
      // What the pane holds, ages left out: they tick on.
      const held = async () => (await app.pane.branches()).map((branch) => [branch.name, branch.selected, branch.expanded, branch.entries.map((entry) => [entry.id, entry.shown])]);
      const before = await held();
      assert.equal((await app.pane.shown()).visible, true);
      await app.command("Show or hide the Pane");
      await app.until(async () => !(await app.pane.shown()).visible, "the switch hid the pane");
      const hidden = await app.pane.layout();
      assert.equal(hidden.divider, false, "its divider goes with it");
      assert.equal((await app.pane.shown()).visible, false);
      assert.ok(Math.abs(await app.graph.width() - await app.windowWidth()) <= 1, "the main area takes the window's width");
      assert.equal(await app.graph.title(), run, "with its view as it was");
      await app.command("Show or hide the Pane");
      await app.until(async () => (await app.pane.shown()).visible, "the switch showed the pane again");
      assert.ok(Math.abs((await app.pane.shown()).width - width) <= 2, "at the width it had");
      assert.equal((await app.pane.search()).query, "sort:name", "its search as it was");
      assert.deepEqual(await held(), before, "its selection and expanded branches as they were");
      // The split stays as dragged whatever the main area shows.
      const other = await app.pane.selectBranch("other  master");
      assert.notEqual(other.entries[0].id, run, "another branch's Run");
      await app.graph.titled(other.entries[0].id);
      assert.ok(Math.abs((await app.pane.shown()).width - width) <= 2, "the split stays as dragged");
      // The Detail's left edge sets its width, kept when it opens again.
      await app.graph.node("a:docs");
      await app.graph.open("a:docs", "Artifact docs");
      const wider = await app.detail.widen(80);
      const placed = (await app.detail.placed())!;
      assert.ok(placed.left > 0 && placed.right === 0, "within the main area, at its right edge");
      await app.detail.close();
      await app.detail.closed();
      await app.graph.open("a:docs", "Artifact docs");
      assert.equal((await app.detail.placed())?.width, wider, "it opens again at its dragged width");
    },
  }),

  scenario({
    id: "branch-star",
    // Its app is started again: the phases run one after the other, alone.
    alone: true,
    title: "a branch's star lists it before the others, still when the app starts again, and taking the star away lists it in the search's order again",
    journeys: ["J-BRANCH-STAR"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      const first = request(fixture).runId;
      await addRepository(fixture, "other");
      const second = requests(fixture).find((saved) => saved.runId !== first)!.runId;
      // The other project's Run is the newer: sort:recent lists it first.
      orderRuns(fixture, [first, second]);
      return { fixture, data: null };
    },
    run: async (app) => {
      await app.pane.listed(["other  master", "repo  master"], "the newest Run's branch first");
      await app.pane.star("repo  master");
      await app.pane.listed(["repo  master", "other  master"], "the starred branch first");
    },
    // A later app of this machine's user keeps the star.
    restarted: async (app) => {
      await app.pane.listed(["repo  master", "other  master"], "still starred and first after the app started again");
      await app.pane.star("repo  master");
      await app.pane.listed(["other  master", "repo  master"], "in the search's order again");
    },
  }),

  scenario({
    id: "branch-older-runs",
    title: "an expanded branch's timeline lists its newest Runs, newest first, and Older runs at its end brings the older ones in below the oldest",
    journeys: ["J-BRANCH-RUNS"],
    prepare: async (own) => {
      // More Runs than a timeline lists at first.
      const fixture = createProject(own, [[["app"], "app", "basis = true\n"]]);
      const runs = [savedRuns(fixture)[0].id];
      for (let more = 0; more < 24; more += 1) runs.push(verify(fixture));
      orderRuns(fixture, runs);
      return { fixture, data: { newest: [...runs].reverse() } };
    },
    run: async (app, { data: { newest } }) => {
      const listed = (entries: { id: string }[]) => entries.map((entry) => entry.id);
      await app.pane.selectBranch("repo  master");
      const first = listed((await app.pane.branch("repo  master", (branch) => listed(branch.entries).length > 0, "its timeline listed")).entries);
      assert.ok(first.length > 0 && first.length < newest.length, `the newest Runs listed, not all of them: ${first.length} of ${newest.length}`);
      assert.deepEqual(first, newest.slice(0, first.length), "newest first");
      const all = await app.pane.older("repo  master");
      assert.deepEqual(listed(all.entries), newest, "the older Runs in below the oldest, newest first");
    },
  }),

  scenario({
    id: "pane-search",
    title: "a click in the search field focuses it; free text and key:value terms narrow the branches, and typing a term suggests keys and their values, a click taking one",
    journeys: ["J-PANE-SEARCH", "J-PANE-SUGGEST"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      await addWorktrees(fixture, ["scope-one", "scope-two"]);
      await addRepository(fixture, "other");
      return { fixture, data: null };
    },
    run: async (app) => {
      await app.pane.branch("repo  scope-two");
      await app.pane.focusSearch();
      assert.equal((await app.pane.search()).query, "sort:recent", "the starting query");
      const names = async () => (await app.pane.branches()).map((branch) => branch.name).sort();
      await app.pane.searchFor("sort:recent scope");
      await app.until(async () => JSON.stringify(await names()) === JSON.stringify(["repo  scope-one", "repo  scope-two"]),
        "free text narrowed the branches to the scope ones");
      await app.pane.searchFor("sort:recent branch:one");
      await app.until(async () => JSON.stringify(await names()) === JSON.stringify(["repo  scope-one"]), "branch: narrowed to scope-one");
      // A key is suggested, then its values; a click takes each.
      await app.pane.searchFor("sort:recent ");
      await app.pane.typeSearch("rep");
      assert.equal(await app.pane.takeSuggestion("repo:"), "sort:recent repo:");
      assert.equal(await app.pane.takeSuggestion("repo:other"), "sort:recent repo:other ");
      await app.until(async () => JSON.stringify(await names()) === JSON.stringify(["other  master"]),
        "repo:other narrowed the branches to its own");
      await app.pane.typeSearch("bra");
      await app.until(async () => (await app.pane.search()).suggestions.includes("branch:"), "the branch key suggested");
      await app.pane.leaveSearch();
      assert.deepEqual((await app.pane.search()).suggestions, [], "the suggestions close as the field loses focus");
    },
  }),

  scenario({
    id: "waiting-live",
    title: "Review lists waiting Human reviews with a live unfiltered badge, and opens a listed request",
    journeys: ["J-BAR", "J-REVIEW-LIST", "J-REVIEW-FILTER", "J-LIVE"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      const older = request(fixture);
      await addRepository(fixture, "other");
      const newer = requests(fixture).find((saved) => saved.evalId === "release/signoff" && saved.id !== older.id)!;
      orderRuns(fixture, [older.runId, newer.runId]);
      return { fixture, data: { older: older.id, newer: newer.id } };
    },
    run: async (app, { fixture, data: { older, newer } }) => {
      await app.mode("Review");
      await app.waitingBadge(2);
      assert.equal(await app.review.filter(), "type:human waiting");
      await app.until(async () => (await app.review.list()).length === 2, "both waiting reviews listed");
      // A filter the library cannot read lists nothing and says why.
      await app.review.filterFor("type:robot");
      await app.until(async () => (await app.review.list()).length === 0 && /Unknown kind `robot`/.test(await app.review.notice()), "the unreadable filter lists nothing and says why");
      assert.equal(await app.review.listEmpty(), "No review matches the filter.");
      await app.review.filterFor("type:human repo:other");
      await app.until(async () => (await app.review.list()).map((row) => row.id).join() === newer, "only the other repository's review matches the new filter");
      await app.waitingBadge(2);
      assert.equal(artifactize(fixture, ["request", "claim", older, "--reviewer", "bob"]).status, 0);
      await app.waitingBadge(1);
      // Edited back to the waiting reviews, the one bob claimed is no longer listed.
      await app.review.filterFor("type:human waiting");
      await app.until(async () => (await app.review.list()).map((row) => row.id).join() === newer, "the claimed review no longer waits");
      await app.review.openListed(newer);
      assert.match(await app.review.debugState(), new RegExp(`^Request ${newer} `));
    },
  }),
  scenario<{ folder: NativePath; review: string }>({
    id: "review-removed-folder",
    title: "a waiting review disappears when its folder goes even if Verify no longer listed the folder",
    journeys: ["J-LIVE", "J-REVIEW-LIST"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      const folder = nativePath(fixture.root, "folder-only");
      declareRelease(fixture, {}, folder, "# A second release\n");
      removeTree(nativePath(folder, ".git")); // This workspace is a folder, not a Git worktree.
      const run = await verifyWaiting(fixture, folder);
      const review = requests(fixture).find((saved) => saved.runId === run && saved.evalId === "release/signoff")!.id;
      return { fixture, data: { folder, review } };
    },
    run: async (app, { data: { folder, review } }) => {
      const visible = async () => (await app.pane.branches()).some((branch) => branch.label.includes("folder-only"));
      await app.until(visible, "the folder is listed before its declarations are removed");
      removeTree(nativePath(folder, ".artifactize"));
      await app.until(async () => !await visible(), "Verify no longer lists the folder");
      await app.mode("Review");
      await app.until(async () => (await app.review.list()).some((row) => row.id === review), "the folder's review still waits as its folder still exists");
      removeTree(folder);
      await app.until(async () => !(await app.review.list()).some((row) => row.id === review), "Review dropped the missing folder's waiting request on its own");
      await app.waitingBadge(1);
    },
  }),
];
