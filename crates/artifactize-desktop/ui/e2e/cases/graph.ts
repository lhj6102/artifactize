// The graphs of a Run and of a Current: verdicts on nodes, the Detail of an Artifact and of an
// eval, finding and starring, wholes and their parts, a graph too large to draw at once, the
// camera, and an Artifact's own tools.
import assert from "node:assert/strict";
import { CHAT_MODEL, CONTINUE_ANSWER, CONTINUE_PROMPT, GRAPH_ANSWER, GRAPH_GATE, GRAPH_PROMPT, MOVE_ANSWER, MOVE_FILE, MOVE_MARKER, MOVE_PROMPT, installChatProvider } from "../fixtures/chat.ts";
import { declareArtifact } from "../fixtures/declarations.ts";
import { REVIEW_FINDING, REVIEW_MODEL, addWorktree, createChecking, createCheckedOnce, createLarge, createParts, createRelease, createReviewed, createStates, git, request, savedRuns } from "../fixtures/repo.ts";
import { nativePath, removeTree, writeText } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

export const scenarios: Scenario[] = [
  scenario({
    id: "graph-verdicts",
    title: "each node of a Run says its verdict by its icon and its name read aloud, the Pane's entry names the Run, and Run details in the stage header opens the Run's Detail with its outcome",
    journeys: ["J-GRAPH-COLOURS", "J-MAIN-RUN"],
    prepare: async (own) => {
      const fixture = await createStates(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      const expected: Record<string, [string, RegExp]> = {
        "a:red": ["red", /^red: RED/], "a:crash": ["error", /^crash: ERROR/], "a:down": ["blocked", /^down: blocked/],
        "a:fine": ["green", /^fine: GREEN/], "a:gate": ["active", /^gate: in progress/], "a:later": ["waiting", /^later: waiting/],
      };
      const icons = new Map<string, string>();
      for (const [id, [light, label]] of Object.entries(expected)) {
        const node = await app.graph.node(id, (shown) => shown.light === light && label.test(shown.label), `${id} drawn ${light}`);
        assert.notEqual(node.icon, "", `${id} shows its verdict's icon`);
        icons.set(light, node.icon);
      }
      assert.equal(new Set(icons.values()).size, icons.size, "one icon per verdict");
      // The Pane's shown entry names the Run; the stage header holds tools only (J-STAGE).
      assert.match(await app.graph.title(), /^run-/);
      const header = await app.graph.header();
      assert.match(header, /Run details/);
      assert.match(header, /Open in Chat/);
      assert.doesNotMatch(header, /run-|WAITING|red\/check/, `the stage header names and rates nothing: ${header}`);
      await app.graph.openRunDetail();
      const outcome = await app.detail.section("Outcome");
      assert.match(outcome, /Status WAITING.HUMAN/, `the Run's status in its outcome: ${outcome}`);
      assert.match(outcome, /red\/check/, `with the evals that failed: ${outcome}`);
    },
  }),

  scenario({
    id: "eval-summary",
    title: "an eval's tab shows its Summary: its status with what running it took and spent, its instruction and its result, its issues as numbered items each led by its priority",
    journeys: ["J-DETAIL-SUMMARY"],
    prepare: async (own) => {
      const fixture = await createReviewed(own);
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    run: async (app, { data: { run } }) => {
      await app.pane.choose("repo  master", run);
      await app.graph.open("a:report", "Artifact report");
      await app.detail.openEval("report/agent");
      const summary = await app.detail.summaryUntil((shown) => shown.status !== "", "the eval's Summary");
      assert.match(summary.status, /^GREEN/, "its status");
      assert.ok(summary.status.includes(REVIEW_MODEL) && /48(\.\d)?k/.test(summary.status), `what it took and spent, its model and tokens: ${summary.status}`);
      assert.equal(summary.instruction, "Review the receipt.");
      const result = await app.detail.section("Result");
      assert.ok(/issues\s*P2\s*The receipt reads right; its date could be clearer\./.test(result), `its result's issues, each by its priority: ${result}`);
      // Its Agent log shows the saved transcript.
      await app.detail.view("Agent log", (text) => text.includes(REVIEW_FINDING), "the Agent log shows the transcript");
    },
  }),

  scenario({
    id: "graph-detail",
    title: "a click on an Artifact docks its Detail at the graph's right edge without moving the camera; its tab lists its evals and folded Neighbours, an eval's tab shows its Summary, and the background closes it",
    journeys: ["J-DETAIL", "J-DETAIL-NEIGHBOURS", "J-DETAIL-SECTIONS", "J-GRAPH-CAMERA"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      await app.graph.node("a:ship");
      const fitted = await app.graph.camera();
      const camera = await app.graph.cameraTool("Zoom out");
      assert.notEqual(camera, fitted, "Detail is opened from a non-fit camera");
      await app.graph.open("a:ship", "Artifact ship");
      let detail = await app.detail.until((shown) => shown.docked, "the Detail docked at the graph's right edge");
      assert.deepEqual(detail.evals.map((row) => row.name), ["check"]);
      assert.deepEqual(detail.tabs, [], "the Artifact's own tab has no chips");
      assert.equal(await app.graph.camera(), camera, "opening the Detail leaves the camera where it was");
      // Its sections: Neighbours folded until unfolded, drawing ship between what it uses and what uses it.
      assert.equal(detail.sections.find((section) => section.name === "Neighbours")?.open, false, "Neighbours start folded");
      detail = await app.detail.toggleSection("Neighbours");
      const neighbours = await app.detail.neighbours();
      assert.equal(neighbours.find((node) => node.id === "a:release")?.side, "left", "ship's eval uses release: on its left");
      await app.detail.neighbourEdge("a:release", "a:ship");
      // Folded again, the drawing goes and its header stays.
      detail = await app.detail.toggleSection("Neighbours");
      await app.until(async () => !(await app.detail.neighboursDrawn()), "the Neighbours drawing gone");
      assert.ok(detail.sections.some((section) => section.name === "Neighbours"), "its header stays");
      detail = await app.detail.toggleSection("Neighbours");
      // Unfolded as the person left it in the next Detail too.
      await app.graph.open("a:release", "Artifact release");
      detail = await app.detail.until((shown) => shown.sections.some((section) => section.name === "Neighbours"), "release's Detail");
      assert.equal(detail.sections.find((section) => section.name === "Neighbours")?.open, true, "Neighbours unfolded in release's Detail as left");
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.open("a:ship", "Artifact ship");
      await app.detail.openNeighbour("a:release", "Artifact release");
      // A click on another Artifact shows its Detail instead; an eval's row shows its tab.
      await app.graph.open("a:docs", "Artifact docs");
      detail = await app.detail.openEval("docs/lint");
      assert.deepEqual(detail.tabs.map((tab) => [tab.name, tab.selected]), [["docs", false], ["lint", true]], "the Artifact's chip, then the eval's, chosen");
      const summary = await app.detail.summaryUntil((shown) => shown.status !== "", "the eval's Summary");
      assert.match(summary.status, /^GREEN/, "its status");
      assert.equal(summary.instruction, "Check docs.", "its instruction");
      await app.graph.clickBackground();
      await app.detail.closed();
    },
  }),

  scenario({
    id: "graph-find-star",
    // Its app is started again: the phases run one after the other, alone.
    alone: true,
    title: "Find lists the Artifacts matching what is typed and opens the chosen one's Detail in the middle of the graph; the star beside a Detail's title stars an Artifact, which Find lists with nothing typed, still when the app starts again",
    journeys: ["J-GRAPH-FIND", "J-DETAIL-STAR"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      await app.graph.node("a:docs");
      const opened = await app.graph.openFind();
      assert.equal(opened.value, "");
      const found = await app.graph.findText("do", (shown) => shown.entries.some((entry) => entry.name === "docs"), "docs found");
      assert.ok(found.entries.every((entry) => entry.name.includes("do")), "only what matches");
      await app.graph.chooseFound("docs", "Artifact docs");
      await app.graph.until(async () => {
        const offCentre = await app.graph.offCentre("a:docs");
        return !!offCentre && Math.abs(offCentre.x) <= 24 && Math.abs(offCentre.y) <= 24;
      }, "docs in the middle of the graph left of the Detail");
      // A star on the Detail stars docs; Find with nothing typed lists it.
      await app.detail.star();
      await app.graph.clickBackground();
      await app.detail.closed();
    },
    // A later app keeps the star: Find with nothing typed lists docs.
    restarted: async (app) => {
      await app.graph.node("a:docs");
      const favorites = await app.graph.openFind((shown) => shown.entries.length > 0, "the find field opened listing the favorites");
      assert.deepEqual(favorites.entries.map((entry) => entry.name), ["docs"], "the starred Artifact listed with nothing typed");
      await app.graph.chooseFound("docs", "Artifact docs");
      await app.detail.until((shown) => shown.starred, "docs's Detail shows its star");
      await app.detail.star();
      assert.equal((await app.detail.read())?.starred, false, "the star taken away");
    },
  }),

  scenario({
    id: "graph-wholes",
    title: "an Artifact with parts is a box around its parts' nodes, nested as deep as they go, a shared part in each of its wholes; a click on the box or a part opens its Detail, and Minimize draws the whole as one node that Expand opens again",
    journeys: ["J-GRAPH-PARTS", "J-GRAPH-MINIMIZE"],
    prepare: async (own) => {
      const fixture = createParts(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      // suite holds app and site, app holds kit; site is a part of docs too.
      // Fit may land below the legible zoom on a narrower window; zoom in until nodes draw.
      await app.graph.until(async () => (await app.graph.nodes()).length > 0 || await app.graph.paints(), "the graph drew");
      for (let step = 0; step < 6 && (await app.graph.nodes()).length === 0; step += 1) await app.graph.cameraTool("Zoom in");
      await app.graph.node("a:suite");
      const ids = (await app.graph.nodes()).map((node) => node.id).sort();
      for (const id of ["a:suite", "a:suite/a:app", "a:suite/a:app/a:kit", "a:suite/a:site", "a:docs/a:site", "a:lib", "a:guide"]) assert.ok(ids.includes(id), `${id} drawn: ${ids.join(" ")}`);
      const suite = (await app.graph.box("a:suite"))!;
      const kit = (await app.graph.box("a:suite/a:app/a:kit"))!;
      assert.ok(kit.x >= suite.x && kit.x + kit.width <= suite.x + suite.width && kit.y >= suite.y && kit.y + kit.height <= suite.y + suite.height,
        "the nested part lies inside its outer whole's box");
      await app.graph.open("a:suite/a:app/a:kit", "Artifact kit");
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.open("a:suite", "Artifact suite");
      await app.graph.clickBackground();
      await app.detail.closed();
      await app.graph.minimize("a:suite", "suite", true);
      const minimized = await app.graph.node("a:suite");
      assert.match(minimized.label, /, minimized, [2-9] parts$/, `a node that says how many parts it has: ${minimized.label}`);
      // Of a node's size: as an ordinary node, not the box it was.
      const [small, lib] = [(await app.graph.box("a:suite"))!, (await app.graph.box("a:lib"))!];
      assert.ok(small.height <= lib.height * 1.5 && small.width <= lib.width * 1.5 && small.height < suite.height / 2, `suite drawn at a node's size: ${JSON.stringify({ small, lib, suite })}`);
      await app.graph.gone("a:suite/a:app");
      await app.graph.minimize("a:suite", "suite", false);
      await app.graph.node("a:suite/a:app");
    },
  }),

  scenario({
    id: "graph-large",
    title: "a graph too large to draw at once first shows whole, painted as blocks; zooming in draws the nodes it brings into view, the Navigator then shows and moves the camera to a place clicked in it, drawing the Artifacts the move brings into view, and Fit shows the whole again",
    journeys: ["J-GRAPH-LARGE", "J-GRAPH-NAVIGATOR", "J-GRAPH-CAMERA"],
    timeout: 300000,
    prepare: async (own) => {
      const fixture = createLarge(own, 1200);
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    run: async (app, { data: { run } }) => {
      await app.pane.choose("repo  master", run);
      await app.graph.titled(run);
      await app.graph.camera();
      // Zoomed out so far, every Artifact is a painted block: the canvas paints them (#475).
      await app.graph.until(() => app.graph.paints(), "the large graph paints its blocks");
      assert.equal(await app.graph.navigatorShown(), false, "the whole graph in view: no Navigator");
      const whole = await app.graph.zoom();
      // Zooming in, about the middle of the view, brings named nodes into view.
      let steps = 0;
      for (; steps < 12 && (await app.graph.nodes()).length === 0; steps += 1) await app.graph.cameraTool("Zoom in");
      const drawn = await app.graph.nodes();
      assert.ok(drawn.length > 0 && drawn.length < 1200, `the nodes in view are drawn: ${drawn.length}`);
      const zoom = await app.graph.zoom();
      assert.ok(zoom > whole, "zoomed in");
      // The Artifact drawn nearest the middle of the view. Zoomed out again as far, the
      // Artifacts are blocks, and a click on the block where it was drawn opens its Detail.
      const middle = await app.graph.central();
      for (let step = 0; step < steps; step += 1) await app.graph.cameraTool("Zoom out");
      assert.equal(await app.graph.zoom(), whole, "zoomed out as far as it was zoomed in");
      await app.graph.until(async () => (await app.graph.nodes()).length === 0 && await app.graph.paints(), "the Artifacts are painted as blocks again");
      await app.graph.clickFromMiddle((middle.x * whole) / zoom, (middle.y * whole) / zoom);
      await app.detail.until((shown) => shown.title === `Artifact ${middle.id.replace(/^a:/, "")} [basis]`, `the block where ${middle.id} is drawn opened that Artifact's Detail`);
      await app.graph.clickBackground();
      await app.detail.closed();
      for (let step = 0; step < steps; step += 1) await app.graph.cameraTool("Zoom in");
      await app.graph.until(() => app.graph.navigatorShown(), "part of the graph lies outside the view: the Navigator shows");
      assert.equal(await app.graph.zoom(), zoom);
      // The Navigator's map shows the whole graph in small: blocks all over its drawing.
      const blocks = await app.graph.navigatorBlocks();
      assert.ok(blocks.every((painted) => painted > 20), `the Navigator's map paints the graph's Artifacts as blocks in each quarter of it: ${blocks.join(", ")}`);
      // The place the Navigator frames now is the place in the map of the Artifact in the
      // middle of the view.
      const place = await app.graph.navigatorFrame();
      const near = (at: { x: number; y: number } | null) => !!at && Math.hypot(at.x - middle.x, at.y - middle.y) <= place.step;
      // A click elsewhere in the map moves the camera there, that Artifact out of the middle,
      // and draws the Artifacts the move brings into view.
      const before = new Set((await app.graph.nodes()).map((node) => node.id));
      await app.graph.navigate(0.3, 0.6);
      assert.ok(!near(await app.graph.offCentre(middle.id)), `${middle.id} left the middle of the view`);
      /** The Artifact nearest the middle of the view of those not drawn before the move. */
      const brought = async () => {
        let nearest: { id: string; off: number } | undefined;
        for (const node of (await app.graph.nodes()).filter((each) => !before.has(each.id))) {
          const at = await app.graph.offCentre(node.id);
          if (at && (!nearest || Math.hypot(at.x, at.y) < nearest.off)) nearest = { id: node.id, off: Math.hypot(at.x, at.y) };
        }
        return nearest?.id;
      };
      let shown: string | undefined;
      await app.graph.until(async () => (shown = await brought()) !== undefined, "the move draws Artifacts it brought into view");
      await app.graph.node(shown!, () => true, `${shown}, brought into view by the move, drawn where it shows`);
      // A click on its place in the map brings it back to the middle of the view, as near as
      // one pixel of the map says.
      await app.graph.navigate(place.fx, place.fy);
      await app.graph.until(async () => near(await app.graph.offCentre(middle.id)), `the place clicked in the Navigator, ${middle.id}'s, is in the middle of the view`);
      assert.equal(await app.graph.zoom(), zoom, "the Navigator keeps the zoom");
      await app.graph.cameraTool("Fit");
      assert.equal(await app.graph.zoom(), whole, "Fit shows the whole graph again");
      await app.graph.until(async () => !(await app.graph.navigatorShown()), "the whole graph in view: the Navigator gone");
    },
  }),

  scenario({
    id: "chat-without-target",
    title: "started with graph --repo and no provider or Chat agent configured, New chat creates a targetless session whose Graph still scans the branch's Current",
    journeys: ["J-LAUNCH", "J-CHAT-NEW", "J-CHAT-GRAPH", "J-CHAT-MODEL"],
    prepare: async (own) => {
      const fixture = createChecking(own);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: null };
    },
    run: async (app) => {
      const starting = await app.chat.createFromDialog("repo / master");
      assert.match(starting, /No agent or provider is ready/, `targetless New chat explains why no target is set: ${starting}`);
      assert.equal(await app.chat.selected(), "Choose a model");
      await app.chat.show("Graph");
      await app.graph.node("a:other", (node) => node.label === "other: would run (1 eval)", "a targetless session's Graph still scans its Current");
    },
  }),

  scenario({
    id: "current-scan",
    title: "started with graph --repo on a branch without a session it offers New chat there, whose Graph shows the branch's Current: its graph drawn at once, Artifacts being checked until their fingerprints are in, then coloured in place by what verify would do; the chat floats over it with the reply card of a message sent from Graph, and Conversation finds text in place",
    journeys: ["J-LAUNCH", "J-CHAT-GRAPH", "J-STAGE", "J-CHAT-FIND", "J-CHAT"],
    prepare: async (own) => {
      const fixture = createChecking(own);
      await installChatProvider(own, fixture);
      return { fixture, args: ["graph", "--repo", fixture.repo], data: { gate: fixture.gate } };
    },
    run: async (app, { fixture, data: { gate } }) => {
      await app.chat.createFromDialog("repo / master");
      await app.chat.show("Graph");
      await app.graph.node("a:slow", (node) => node.label === "slow: checking", "slow is still being checked");
      await app.graph.node("a:quick", (node) => node.label === "quick: checking", "quick, which reads slow, too");
      await app.graph.node("a:other", (node) => node.label === "other: would run (1 eval)", "other's verdict is in");
      assert.match(await app.graph.scan(), /^Checking, 2 Artifacts left/, "the header counts what is still being checked");
      writeText(gate, "go");
      await app.graph.node("a:slow", (node) => node.label === "slow: would run (1 eval)", "slow coloured in place once its fingerprint is in");
      await app.graph.node("a:quick", (node) => /^quick: (would run|waiting)/.test(node.label), "quick too");
      // Every Artifact checked, the scan still ends after its last one: the header then says when.
      await app.graph.until(async () => /^Scanned at /.test(await app.graph.scan()), "the scan ended");
      // The stage header: the view tabs, then the graph's tools and the Current's, naming nothing.
      const header = await app.graph.header();
      for (const tool of ["Conversation", "Graph", "Find", "Zoom in", "Fit", "Refresh"]) assert.ok(header.includes(tool), `${tool} in the stage header: ${header}`);
      assert.doesNotMatch(header, /New chat|master|Scanned/, `the stage header names and rates nothing: ${header}`);
      // The chat floats over the graph's bottom, centred, with no reply card before a message.
      const float = await app.chat.floating();
      assert.ok(float, "the chat floats over the graph");
      assert.ok(float.chat.bottom < float.canvas.bottom && float.canvas.bottom - float.chat.bottom <= 16, `12px above the canvas's bottom: ${JSON.stringify(float)}`);
      assert.ok(Math.abs((float.chat.left - float.canvas.left) - (float.canvas.right - float.chat.right)) <= 2, `centred in the canvas: ${JSON.stringify(float)}`);
      assert.equal(await app.chat.reply(), null);
      await app.chat.model(CHAT_MODEL);
      await app.chat.send(GRAPH_PROMPT, false);
      await app.chat.working();
      await app.chat.viewShown("Graph");
      // The message sent from Graph heads the reply card as the reply streams.
      await app.until(async () => (await app.chat.reply())?.said === GRAPH_PROMPT, "the reply card shows the message sent from Graph");
      await app.graph.node("a:slow");
      writeText(nativePath(fixture.root, GRAPH_GATE), "answer");
      await app.chat.finished();
      await app.until(async () => (await app.chat.reply())?.text.includes(GRAPH_ANSWER) === true, "the reply streamed into the card");
      await app.chat.viewShown("Graph");
      await app.chat.toggleReply();
      assert.equal((await app.chat.reply())?.text, "", "folded to its head");
      await app.chat.toggleReply();
      // Conversation holds the whole turn; Find marks where a text appears and steps through it.
      await app.chat.show("Conversation");
      await app.chat.says(GRAPH_ANSWER);
      assert.equal(await app.chat.find("SEE"), "1 of 2", "ignoring case: you see, I see");
      assert.equal(await app.chat.findStep(), "2 of 2");
      assert.equal(await app.chat.findStep(true), "1 of 2");
      assert.equal(await app.chat.find("no such words"), "No match");
      await app.chat.closeFind();
      // The card was the Graph view's: gone once Conversation was shown.
      await app.chat.show("Graph");
      assert.equal(await app.chat.reply(), null);
      // A project folds from its row and unfolds again.
      await app.chat.toggleProject("repo");
      assert.deepEqual(await app.chat.projects(), [{ name: "repo", open: false }]);
      await app.chat.toggleProject("repo");
      await app.chat.create();
      await app.chat.viewShown("Conversation");
      await app.chat.openSaved(GRAPH_PROMPT);
      await app.chat.viewShown("Graph");
      await app.graph.node("a:slow");
    },
  }),

  scenario({
    id: "chat-move",
    title: "Move takes a Chat session to another checkout: its conversation kept, its row naming the new branch and its Graph that branch's Current; a project's row starts New chat on that project",
    journeys: ["J-CHAT-MOVE", "J-CHAT-NEW", "J-CHAT"],
    prepare: async (own) => {
      const fixture = await createCheckedOnce(own);
      git(fixture, fixture.repo, "add", "--all");
      git(fixture, fixture.repo, "commit", "-qm", "Fixture inputs");
      addWorktree(fixture, "elsewhere");
      const other = nativePath(fixture.root, "elsewhere");
      removeTree(nativePath(other, ".artifactize"));
      declareArtifact(other, ["elsewhere-only"], "elsewhere-only", "basis = true\n");
      writeText(nativePath(other, "elsewhere-only", "marker.txt"), `${MOVE_MARKER}\n`);
      await installChatProvider(own, fixture);
      return { fixture, data: null };
    },
    run: async (app) => {
      await app.pane.selectBranch("repo  master");
      await app.mode("Chat");
      await app.chat.create();
      assert.equal(await app.chat.shownBranch(), "master");
      await app.chat.model(CHAT_MODEL);
      await app.chat.send(CONTINUE_PROMPT);
      await app.chat.says(CONTINUE_ANSWER);
      await app.chat.finished();
      await app.chat.show("Graph");
      await app.graph.node("a:slow");
      await app.chat.move("repo / elsewhere", "elsewhere");
      // The Graph view follows the session to the new branch's Current.
      await app.chat.viewShown("Graph");
      await app.graph.node("a:elsewhere-only");
      await app.graph.until(async () => (await app.graph.nodes()).every(({ id }) => id === "a:elsewhere-only"), "only elsewhere's declarations in the moved session's Current");
      // The earlier conversation is kept, and subsequent turns work in the moved checkout.
      await app.chat.show("Conversation");
      await app.chat.says(CONTINUE_ANSWER);
      await app.chat.send(MOVE_PROMPT);
      await app.chat.says(MOVE_FILE);
      await app.chat.says(MOVE_ANSWER);
      await app.chat.finished();
      // From the project's row, New chat starts on that project's newest session's branch.
      await app.chat.newChatIn("repo");
      await app.until(async () => await app.chat.newChatBranch() === "repo / elsewhere", "New chat starts on the project's newest session's branch");
    },
  }),

  scenario({
    id: "current-chosen",
    title: "Open in Chat from a Run shows its branch's latest session in Graph, which scans the worktree as it is now: its Artifacts checked, then coloured in place by what verify would do, a result whose input changed since the Run to run again",
    journeys: ["J-MAIN-CURRENT", "J-OPEN-CHAT"],
    prepare: async (own) => {
      const fixture = await createCheckedOnce(own);
      git(fixture, fixture.repo, "add", "--all");
      git(fixture, fixture.repo, "commit", "-qm", "Fixture inputs");
      addWorktree(fixture, "elsewhere");
      const other = nativePath(fixture.root, "elsewhere");
      removeTree(nativePath(other, ".artifactize"));
      declareArtifact(other, ["elsewhere-only"], "elsewhere-only", "basis = true\n");
      // quick changes after the Run: its saved result is stale.
      writeText(nativePath(fixture.repo, "quick", "changed.md"), "quick changed after the Run\n");
      await installChatProvider(own, fixture);
      return { fixture, data: { gate: fixture.gate, run: request(fixture, "other/signoff").runId } };
    },
    run: async (app, { fixture, data: { gate, run } }) => {
      await app.pane.selectBranch("repo  master");
      await app.mode("Chat");
      await app.chat.create(); // An older session on the original branch.
      await app.chat.create();
      await app.chat.model(CHAT_MODEL);
      await app.chat.send(CONTINUE_PROMPT);
      await app.chat.says(CONTINUE_ANSWER);
      await app.chat.finished();
      await app.graph.titled(CONTINUE_PROMPT);

      // Newer globally, but on another branch. Leave this empty Chat selected before Verify.
      await app.mode("Verify");
      await app.pane.selectBranch("repo  elsewhere");
      await app.mode("Chat");
      await app.chat.create();
      await app.graph.titled("New chat");
      assert.equal(await app.chat.shownBranch(), "elsewhere", "the newer session is on the other branch");
      await app.chat.show("Graph");
      await app.graph.node("a:elsewhere-only");
      await app.graph.until(async () => /^Scanned at /.test(await app.graph.scan()), "elsewhere's Current finished scanning");
      assert.deepEqual((await app.graph.nodes()).map(({ id }) => id), ["a:elsewhere-only"], "no master declarations in elsewhere's Current");

      await app.mode("Verify");
      await app.pane.selectBranch("repo  master");
      await app.graph.titled(run);
      await app.graph.openInChat();
      await app.graph.titled(CONTINUE_PROMPT); // Neither the globally newest nor the older master session.
      await app.chat.viewShown("Graph");
      assert.equal(await app.chat.selected(), CHAT_MODEL);
      await app.graph.until(async () => (await app.graph.scan()) !== "", "the Run's branch is being scanned in Chat");
      await app.graph.node("a:slow", (node) => node.label === "slow: checking", "slow is being checked");
      writeText(gate, "go");
      // slow and other did not change since the Run: their results would be reused; quick did.
      await app.graph.node("a:slow", (node) => /^slow: reused/.test(node.label), "slow coloured in place as reused once its fingerprint is in");
      await app.graph.node("a:other", (node) => /^other: reused/.test(node.label), "other's sign-off would be reused");
      await app.graph.node("a:quick", (node) => node.label === "quick: would run (1 eval)", "quick, changed since the Run, would run again");
      // Every Artifact checked, the scan still ends after its last one: the header then says when.
      await app.graph.until(async () => /^Scanned at /.test(await app.graph.scan()), "the scan ended");
      await app.chat.send(GRAPH_PROMPT, false);
      await app.chat.working();
      await app.chat.viewShown("Graph");
      await app.graph.node("a:slow");
      writeText(nativePath(fixture.root, GRAPH_GATE), "answer");
      await app.chat.finished();
      await app.chat.viewShown("Graph");
      await app.chat.show("Conversation");
      await app.chat.says(GRAPH_ANSWER);
    },
  }),

  scenario({
    id: "detail-tools",
    title: "an Artifact's Detail lists the Human tools it declares; a click runs one at once in its worktree, with no claim, its output and end in the Detail's Tool log",
    journeys: ["J-DETAIL-TOOLS"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.graph.open("a:release", "Artifact release");
      // The worktree changed since the shown Run: the tool reads it as it is now.
      writeText(nativePath(fixture.repo, "release", "notes.md"), "# Release notes\n- Changed since the Run.\n");
      await app.detail.runTool("release.notes");
      await app.detail.toolLogUntil((rows) => rows.at(-1) === "■ exit 0", "notes ran to its end");
      assert.deepEqual(await app.detail.toolLog(), ["▶ HH:MM:SS · release.notes · release", "# Release notes", "- Changed since the Run.", "■ exit 0"]);
      assert.equal(request(fixture).claim, null, "no claim");
      assert.equal(request(fixture).status, "DEFERRED", "and no review's evidence");
    },
  }),
];
