// The workbench: the Git layer's history of the chosen worktree and a new worktree from one of
// its commits; each worktree's one terminal floating over the graph's left side, which keeps
// running when collapsed, left or closed, follows its worktree through a branch change, ends with
// it, offers what it can restore once it was gone and the worktree its agent moved to; and the
// terminal panel's width, kept like the Detail's. Every case owns its fixture.
import assert from "node:assert/strict";
import { addFolder, agentRecorded, agentRuns, branchNames, createWorkbench, releaseAgent, type Workbench } from "../fixture.ts";
import { git } from "../environment.ts";
import { afterRender, fill, press } from "../keys.ts";
import { needing, scenario, servedBy, type Prepared, type Scenario } from "../scenario.ts";
import { choosePaletteCommand } from "../palette-commands.ts";
import { sessionEndedByApp } from "../observation.ts";
import {
  agentStarts, asNativePath, childDirectories, exists, heldConnections, heldEndpoint, nameOf, nativePath, primaryModifier, writeText, type NativePath,
} from "../platform.ts";
import { visibleCenter } from "../pointer.ts";
import { camera, detail, FIND, findField, offCentre, TERMINAL } from "../graph-view.ts";
import { branch, branches, entriesFocused, listed, mainFocused, mainTitle, selectBranch } from "../pane.ts";
import {
  collapse, expand, gitLayer, TAB, newWorktreeFrom, printed, screenShows, screenText, shows, terminal, terminalOf, typeLine, type Commit,
} from "../workbench-view.ts";

/** The workbench's repository, started on its master branch. */
async function prepare() {
  const fixture = createWorkbench();
  return { fixture, args: ["--repo", fixture.repo], env: fixture.env, data: fixture };
}

/** Let the agent run with `args` in the workbench's master finish, or move, once it has started
 * and its terminal has recorded it as the program in the foreground, which a restore offer is
 * made from. */
async function finishAgent(browser: WebdriverIO.Browser, fixture: Workbench, args: string): Promise<void> {
  const started = () => agentStarts(fixture.runs).at(-1)?.args === args;
  await browser.waitUntil(() => started() && agentRecorded(fixture.repo, args),
    { timeout: 30000, timeoutMsg: `the agent run with ${args} started and its terminal recorded it` });
  releaseAgent(fixture);
}

/** The workbench with a loopback endpoint that a program in a terminal holds open until it ends
 * ([`heldEndpoint`]): its port and the record of its connections. */
type Holding = Workbench & { held: { port: number; record: NativePath } };
async function prepareHolding(): Promise<Prepared<Holding>> {
  const prepared = await prepare();
  const record = nativePath(prepared.fixture.root, "held-connections.txt");
  const endpoint = await heldEndpoint(record);
  return servedBy(endpoint, () => ({ ...prepared, data: { ...prepared.data, held: { port: endpoint.port, record } } }));
}

/** Wait until the held endpoint's connections reach `opened` and `ended`. */
function held(browser: WebdriverIO.Browser, fixture: Holding, want: { opened: number; ended: number }, what: string) {
  return browser.waitUntil(() => {
    const seen = heldConnections(fixture.held.record);
    return seen.opened === want.opened && seen.ended === want.ended;
  }, { timeout: 30000, timeoutMsg: what });
}

/** The Git layer's history, once it lists `count` commits. */
async function history(browser: WebdriverIO.Browser, count: number, what: string): Promise<Commit[]> {
  let commits: Commit[] = [];
  await browser.waitUntil(async () => (commits = (await gitLayer(browser)).commits).length === count,
    { timeout: 15000, timeoutMsg: `${what}: ${JSON.stringify(commits)}` });
  return commits;
}
const marked = (commits: Commit[]) => commits.filter((commit) => commit.marked).map((commit) => commit.summary);

/** Wait until the Main area shows the Current of the branch read as `name`. */
const currentOf = (browser: WebdriverIO.Browser, name: string) =>
  browser.waitUntil(async () => (await mainTitle(browser)) === `Current · ${name}`, { timeout: 15000, timeoutMsg: `${name}'s Current shown` });

/** The graph's canvas, and whether the expanded terminal floats over it, inside its box. */
async function overGraph(browser: WebdriverIO.Browser) {
  // The graph is drawn once its worktree's scan arrives, which may come after the terminal.
  await browser.$(".monitor .graph > .canvas").waitForExist({ timeout: 30000, timeoutMsg: "the graph drawn under the terminal" });
  return browser.execute((query) => {
    const panel = document.querySelector(query)?.getBoundingClientRect();
    const canvas = document.querySelector(".monitor .graph > .canvas")!.getBoundingClientRect();
    return { over: !!panel && canvas.left < panel.left && canvas.right > panel.right, canvas: [canvas.left, canvas.width].map(Math.round) };
  }, TERMINAL);
}

/** End the shown terminal's shell as a person does: `exit`, until the panel says it ended. */
async function endShell(browser: WebdriverIO.Browser): Promise<void> {
  await typeLine(browser, "exit");
  await browser.waitUntil(async () => (await terminal(browser))?.notes.some((note) => note.startsWith("The terminal ended")) === true,
    { timeout: 15000, timeoutMsg: "the panel says the terminal ended" });
}

/** Choose another worktree and master, whose terminal is gone, again: a fresh shell with a
 * restore offer. */
async function reopen(browser: WebdriverIO.Browser): Promise<{ text: string; buttons: string[] }> {
  await selectBranch(browser, "repo  side");
  await terminalOf(browser, "repo / side");
  await selectBranch(browser, "repo  master");
  await browser.waitUntil(async () => (await terminal(browser))?.offer !== null && (await terminal(browser))?.heading === "Terminal · repo / master",
    { timeout: 30000, timeoutMsg: "master's terminal offers a restore" });
  return (await terminal(browser))!.offer!;
}

/** The worktree the Git layer says it created last, once it names another than `before`. */
async function created(browser: WebdriverIO.Browser, before = ""): Promise<{ branch: string; folder: string }> {
  let said = "";
  await browser.waitUntil(async () => {
    said = (await gitLayer(browser)).notes.find((note) => note.startsWith("Created ")) ?? "";
    return said !== "" && !said.startsWith(`Created ${before} `);
  }, { timeout: 30000, timeoutMsg: "the Git layer says which worktree it created" });
  const [, branch, folder] = said.match(/^Created (\S+) in (.+?)(?:\.|; it opens once listed\.)$/) ?? [];
  assert.ok(branch && folder, `the Git layer names the branch and the folder: ${said}`);
  return { branch, folder };
}

/** The removal dialog's text, or null while it is closed. */
const removal = (browser: WebdriverIO.Browser) => browser.execute(() => {
  const dialog = [...document.querySelectorAll('[role="dialog"]')].find((each) => each.textContent?.includes("Remove this worktree?"));
  return dialog ? (dialog.textContent ?? "").replace(/\s+/g, " ").trim() : null;
});

/** Ask to remove the shown worktree from the palette, and wait for the dialog. */
async function askRemoval(browser: WebdriverIO.Browser): Promise<string> {
  await browser.$("button*=Commands").click();
  await choosePaletteCommand(browser, "Remove the shown", "Remove the shown worktree…");
  await press(browser, ["Enter"], async () => (await removal(browser)) !== null, "the removal dialog asks");
  return (await removal(browser))!;
}

/** Confirm the removal, and wait for the dialog to close. */
async function confirmRemoval(browser: WebdriverIO.Browser): Promise<void> {
  await browser.$('[role="dialog"]').$("button=Remove").click();
  await browser.waitUntil(async () => (await removal(browser)) === null, { timeout: 15000, timeoutMsg: "the removal dialog closed" });
}

/** Whether the Git layer says Git refused a removal. */
const saysRefused = async (browser: WebdriverIO.Browser) =>
  (await gitLayer(browser)).notes.some((note) => note.startsWith("Git refused to remove the worktree"));

/** Ask to remove the shown worktree, confirm, and wait until the Git layer says Git refused
 * this removal: no refusal shows before it is asked, so the one that shows is its own. */
async function removalRefused(browser: WebdriverIO.Browser, what: string): Promise<void> {
  await browser.waitUntil(async () => !(await saysRefused(browser)), { timeout: 15000, timeoutMsg: `no earlier refusal shows before ${what}` });
  await askRemoval(browser);
  await confirmRemoval(browser);
  await browser.waitUntil(() => saysRefused(browser), { timeout: 30000, timeoutMsg: what });
}

export const scenarios: Scenario[] = [
  // UX journeys: J-LAUNCH J-GIT-LAYER J-WORKTREE-NEW; UX edges: E-GIT-HISTORY E-GIT-WORKTREE-NEW E-TERMINAL-OPEN E-TERMINAL-TYPE
  scenario<Workbench>(
    "workbench-git-history",
    "shows the chosen worktree's history at the far left with its checked-out commit marked, and makes a new worktree from a commit that the Pane lists with its own terminal",
    prepare,
    async (browser, { data: fixture }) => {
      await listed(browser, "repo  master");
      await listed(browser, "repo  side");
      await currentOf(browser, "repo  master");
      // The Git layer at the far left, the Pane beside it, then the Main area.
      const columns = await browser.execute(() => {
        const box = (query: string) => document.querySelector(query)!.getBoundingClientRect();
        const [layer, pane, main] = [box('section[aria-label="Git layer"]'), box('section[aria-label="Pane"]'), box('section.panel[aria-label="Main area"]')];
        return layer.left <= 1 && layer.right <= pane.left + 1 && pane.right <= main.left + 1;
      });
      assert.ok(columns, "the Git layer, the Pane and the Main area from left to right");
      // Every branch's commits, each with its summary, short ID and the branches and tags on it;
      // master's checked-out commit marked.
      const all = await history(browser, 3, "master's history lists the commits of every branch");
      assert.deepEqual(all.map((commit) => commit.summary), ["Side work", "Check the docs twice", "Fixture inputs"], "newest at the top");
      assert.ok(all.every((commit) => /^[0-9a-f]{7}$/.test(commit.short)), `short IDs: ${JSON.stringify(all)}`);
      const refs = Object.fromEntries(all.map((commit) => [commit.summary, commit.refs]));
      assert.deepEqual(refs, { "Check the docs twice": ["master"], "Fixture inputs": ["v1"], "Side work": ["side"] });
      assert.deepEqual(marked(all), ["Check the docs twice"]);

      // Choosing the side worktree shows its history, its own commit marked.
      await selectBranch(browser, "repo  side");
      await currentOf(browser, "repo  side");
      await browser.waitUntil(async () => JSON.stringify(marked((await gitLayer(browser)).commits)) === JSON.stringify(["Side work"]),
        { timeout: 15000, timeoutMsg: "side's checked-out commit marked" });
      await terminalOf(browser, "repo / side");

      // A new worktree from the tagged commit: a new folder beside the primary checkout on a new
      // branch, said where; the Pane lists it and it shows with its own terminal.
      const [branchesBefore, foldersBefore] = [branchNames(fixture), childDirectories(fixture.root)];
      await newWorktreeFrom(browser, "Fixture inputs");
      let said = "";
      await browser.waitUntil(async () => !!(said = (await gitLayer(browser)).notes.find((note) => note.startsWith("Created ")) ?? ""),
        { timeout: 30000, timeoutMsg: "the Git layer says which worktree it created" });
      const [, name, where] = said.match(/^Created (\S+) in (.+?)(?:\.|; it opens once listed\.)$/) ?? [];
      assert.ok(name && where, `the Git layer names the branch and the folder: ${said}`);
      // A branch and a folder that did not exist before, the folder beside the primary checkout.
      assert.ok(!branchesBefore.includes(name) && branchNames(fixture).includes(name), `a new branch ${name}: before ${JSON.stringify(branchesBefore)}`);
      const folders = childDirectories(fixture.root).filter((folder) => !foldersBefore.includes(folder));
      assert.equal(folders.length, 1, `one new folder beside the primary checkout: ${JSON.stringify(folders)}`);
      assert.equal(nameOf(asNativePath(where)), nameOf(folders[0]), "the folder the Git layer names");
      await listed(browser, `repo  ${name}`, 30000);
      await currentOf(browser, `repo  ${name}`);
      assert.deepEqual((await branches(browser)).map((entry) => entry.name).sort(), ["repo  master", `repo  ${name}`, "repo  side"].sort());
      await browser.waitUntil(async () => JSON.stringify(marked((await gitLayer(browser)).commits)) === JSON.stringify(["Fixture inputs"]),
        { timeout: 15000, timeoutMsg: "the new worktree's commit marked" });
      // Its own terminal: a shell in the new folder, taking what is typed.
      await terminalOf(browser, `repo / ${name}`);
      await typeLine(browser, "echo new-tree-shell");
      await printed(browser, "new-tree-shell", "the shell ran what was typed");
    },
  ),

  // UX journeys: J-TERMINAL J-TERMINAL-DETACH J-BRANCHES; UX edges: E-TERMINAL-OPEN E-TERMINAL-TYPE E-TERMINAL-COLLAPSE
  // E-TERMINAL-EXPAND E-TERMINAL-SWITCH E-TERMINAL-BRANCH E-TERMINAL-REMOVED E-PANE-WORKTREES
  scenario<Workbench>(
    "workbench-terminal",
    "opens each worktree's own terminal over the graph and none for a folder outside Git, keeps it running while collapsed or left, follows its worktree through a branch rename and says it ended once the worktree is removed",
    async () => {
      const prepared = await prepare();
      addFolder(prepared.fixture, "plain");
      return prepared;
    },
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      // Opened the first time it is shown: a shell over the graph's left side, the graph beneath.
      await terminalOf(browser, "repo / master");
      const floating = await overGraph(browser);
      assert.ok(floating.over, `the terminal floats over the graph: ${JSON.stringify(floating)}`);
      await typeLine(browser, "echo master-mark");
      await printed(browser, "master-mark", "the shell ran what was typed");

      // Collapsed, it keeps running: what a program prints meanwhile shows once it is expanded.
      const shown = await camera(browser);
      // The program prints once its gate opens, which happens only after the collapse is seen.
      await typeLine(browser, "node -e \"const fs=require('fs');const t=setInterval(()=>{if(fs.existsSync('gate')){clearInterval(t);console.log('while-'+'collapsed');fs.writeFileSync('printed','')}},50)\"");
      await collapse(browser);
      assert.equal(await camera(browser), shown, "the graph as it was");
      assert.deepEqual((await overGraph(browser)).canvas, floating.canvas, "the graph never shrank under the terminal");
      writeText(nativePath(fixture.repo, "gate"), "");
      await browser.waitUntil(() => exists(nativePath(fixture.repo, "printed")), { timeout: 30000, timeoutMsg: "the program printed while collapsed" });
      await expand(browser);
      await printed(browser, "while-collapsed", "what it printed while collapsed");
      assert.ok(shows(await terminal(browser), "master-mark"), "the screen as it was");

      // Another worktree has its own terminal; the first comes back as it was.
      await selectBranch(browser, "repo  side");
      await terminalOf(browser, "repo / side");
      assert.ok(!screenText(await terminal(browser)).includes("master-mark"), "side's own terminal");
      await typeLine(browser, "echo side-mark");
      await printed(browser, "side-mark", "side's shell ran what was typed");
      await selectBranch(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await printed(browser, "while-collapsed", "master's screen as it was");
      await selectBranch(browser, "repo  side");
      await printed(browser, "side-mark", "side's screen as it was");

      // A folder outside Git is no worktree: no terminal, and no history to make one from.
      await selectBranch(browser, "plain");
      await currentOf(browser, "plain");
      assert.equal(await terminal(browser), null, "no terminal panel for the folder");
      assert.equal(await browser.$(TAB).isExisting(), false, "nothing to expand either");
      assert.deepEqual((await gitLayer(browser)).notes, ["This folder is outside Git: it has no history and no worktrees."]);
      await selectBranch(browser, "repo  side");
      await printed(browser, "side-mark", "side's terminal again");

      // The terminal belongs to the worktree: renaming its branch keeps it, for the new entry.
      git(fixture.repo, "-C", fixture.side, "branch", "-m", "side", "renamed");
      await listed(browser, "repo  renamed", 30000);
      await browser.waitUntil(async () => (await terminal(browser))?.heading === "Terminal · repo / renamed",
        { timeout: 15000, timeoutMsg: "the terminal names the worktree's new branch" });
      await selectBranch(browser, "repo  renamed");
      await printed(browser, "side-mark", "the same terminal after the rename");
      assert.equal((await terminal(browser))?.offer, null, "no restore: the terminal never stopped");

      // Removing the worktree ends its terminal: the panel says so, and the Pane drops it. The app
      // removes it, as a running terminal's folder cannot be deleted from outside on every system.
      await askRemoval(browser);
      await confirmRemoval(browser);
      await browser.waitUntil(async () => (await terminal(browser))?.notes.some((note) => note.startsWith("The terminal ended")) === true,
        { timeout: 30000, timeoutMsg: "the panel says the removed worktree's terminal ended" });
      await browser.waitUntil(async () => (await branch(browser, "repo  renamed")) === undefined, { timeout: 30000, timeoutMsg: "the removed worktree gone from the Pane" });
    },
  ),

  // UX journey: J-TERMINAL-DETACH; UX edges: E-TERMINAL-TYPE E-QUIT-CLEAN E-TERMINAL-RELAUNCH
  scenario<Workbench>(
    "workbench-terminal-relaunch",
    "keeps a worktree's terminal and the program in it running when the app quits, and shows it as it was in the next launch",
    prepare,
    async (browser) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await typeLine(browser, "echo before-quit");
      await printed(browser, "before-quit", "the shell ran what was typed");
      // A program that waits for its gate, which opens only in the next launch.
      await typeLine(browser, "node -e \"const fs=require('fs');console.log('waiting-'+'for-gate');const t=setInterval(()=>{if(fs.existsSync('relaunched')){clearInterval(t);console.log('gate-'+'seen')}},50)\"");
      await printed(browser, "waiting-for-gate", "the program waits for its gate");
      // q from the Pane quits at once.
      await selectBranch(browser, "repo  master");
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('section[aria-label="Pane"]')),
        { timeout: 15000, timeoutMsg: "the keyboard in the Pane" });
      await sessionEndedByApp(browser, () => browser.keys(["q"]));
    },
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await printed(browser, "waiting-for-gate", "the screen as it was before the app quit");
      assert.ok(shows(await terminal(browser), "before-quit"));
      assert.equal((await terminal(browser))?.offer, null, "the terminal kept running: nothing to restore");
      // The program that ran through the quit sees its gate open now.
      writeText(nativePath(fixture.repo, "relaunched"), "");
      await printed(browser, "gate-seen", "the program still runs after the relaunch");
      await typeLine(browser, "echo after-relaunch");
      await printed(browser, "after-relaunch", "the same shell takes what is typed");
    },
  ),

  // UX journey: J-TERMINAL-RESTORE; UX edges: E-TERMINAL-RESTORE-OFFER E-TERMINAL-TYPE E-TERMINAL-RESTORE-KEEP
  scenario<Workbench>(
    "workbench-terminal-restore",
    "offers a restore over a fresh shell once a terminal was gone, takes typing while the offer waits, and drops the offer when the fresh shell is kept, running nothing",
    prepare,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      // The agent runs, then the shell ends: the terminal is gone.
      await typeLine(browser, "claude --hello");
      await finishAgent(browser, fixture, "--hello");
      await printed(browser, "agent done --hello", "the agent ran and finished");
      await endShell(browser);
      const offer = await reopen(browser);
      assert.ok(offer.buttons.includes("Keep the fresh shell"), JSON.stringify(offer));
      assert.equal(agentRuns(fixture, "claude").length, 1, "offering runs nothing");
      // The fresh shell takes what is typed while the offer waits.
      await typeLine(browser, "echo fresh-shell");
      await printed(browser, "fresh-shell", "the fresh shell ran what was typed");
      assert.notEqual((await terminal(browser))?.offer, null, "the offer still waits");
      // Keeping the fresh shell drops the offer and runs nothing.
      await browser.$(TERMINAL).$("button=Keep the fresh shell").click();
      await browser.waitUntil(async () => (await terminal(browser))?.offer === null, { timeout: 15000, timeoutMsg: "the offer is gone" });
      await typeLine(browser, "echo kept-shell");
      await printed(browser, "kept-shell", "the fresh shell kept");
      assert.equal(agentRuns(fixture, "claude").length, 1, "keeping the fresh shell ran nothing");
    },
  ),

  // UX journey: J-TERMINAL-RESTORE; UX edges: E-TERMINAL-RESTORE-OFFER E-TERMINAL-RESTORE-RUN
  needing(["terminal-foreground"], scenario<Workbench>(
    "workbench-terminal-restore-run",
    "offers the last command from its directory and Claude Code's latest session, labelled so, or the session its records name, and runs the one chosen there",
    prepare,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      // The agent runs in a folder of the worktree the shell moved into, not the worktree itself.
      await typeLine(browser, "cd docs");
      await typeLine(browser, "claude --hello");
      await finishAgent(browser, fixture, "--hello");
      await printed(browser, "agent done --hello", "the agent ran and finished");
      assert.match(agentRuns(fixture, "claude")[0].cwd, /\/repo\/docs$/, "the agent ran in docs");
      await endShell(browser);
      const offer = await reopen(browser);
      assert.match(offer.text, /Rerun the last command in .*[\\/]repo[\\/]docs: claude --hello Run/, "the agent as it was run, where it ran");
      assert.match(offer.text, /Resume the latest Claude Code session: claude --continue/);
      assert.deepEqual(offer.buttons, ["Run the last command again", "Resume the latest Claude Code session", "Keep the fresh shell"]);
      assert.equal(agentRuns(fixture, "claude").length, 1, "offering runs nothing");
      // Resuming runs the agent's resume command in its directory, and the offer goes.
      await browser.$(TERMINAL).$('button[aria-label="Resume the latest Claude Code session"]').click();
      await browser.waitUntil(() => agentRuns(fixture, "claude").length === 2, { timeout: 30000, timeoutMsg: "the resume command ran" });
      const [first, resumed] = agentRuns(fixture, "claude");
      assert.deepEqual(resumed, { args: "--continue", cwd: first.cwd }, "in the agent's directory, docs");
      assert.equal((await terminal(browser))?.offer, null, "the offer is gone");
      await finishAgent(browser, fixture, "--continue");
      await printed(browser, "agent done --continue", "the resumed agent ran and finished");

      // A session the agent's own records name for the worktree resumes by its ID; rerunning
      // the last command runs it again where it ran.
      await typeLine(browser, "cd ..");
      await typeLine(browser, `claude --resume ${fixture.session}`);
      await finishAgent(browser, fixture, `--resume ${fixture.session}`);
      await printed(browser, `agent done --resume+${fixture.session}`, "the agent ran its session and finished");
      const named = agentRuns(fixture, "claude").at(-1)!;
      assert.match(named.cwd, /\/repo$/, "the session ran in the worktree");
      await endShell(browser);
      const known = await reopen(browser);
      assert.match(known.text, new RegExp(`Resume the Claude Code session: claude --resume ${fixture.session}`));
      assert.deepEqual(known.buttons, ["Run the last command again", "Resume the Claude Code session", "Keep the fresh shell"]);
      const runs = agentRuns(fixture, "claude").length;
      await browser.$(TERMINAL).$('button[aria-label="Run the last command again"]').click();
      await browser.waitUntil(() => agentRuns(fixture, "claude").length === runs + 1, { timeout: 30000, timeoutMsg: "the last command ran again" });
      assert.deepEqual(agentRuns(fixture, "claude").at(-1), { args: `--resume ${fixture.session}`, cwd: named.cwd }, "the same command where it ran");
      await browser.waitUntil(async () => (await terminal(browser))?.offer === null, { timeout: 15000, timeoutMsg: "the offer is gone" });
    },
  )),

  // UX journey: J-TERMINAL-RESTORE; UX edge: E-TERMINAL-RESTORE-OFFER
  needing(["no-terminal-foreground"], scenario<Workbench>(
    "workbench-terminal-restore-shell-only",
    "offers only to keep the fresh shell where the system cannot tell which program ran in the terminal",
    prepare,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await typeLine(browser, "claude --hello");
      await finishAgent(browser, fixture, "--hello");
      await printed(browser, "agent done --hello", "the agent ran and finished");
      await endShell(browser);
      assert.deepEqual((await reopen(browser)).buttons, ["Keep the fresh shell"], "the last command and its resume are unknown");
    },
  )),

  // UX journey: J-TERMINAL-MOVED; UX edges: E-TERMINAL-OPEN E-TERMINAL-MOVED E-TERMINAL-FOLLOW
  needing(["terminal-foreground"], scenario<Workbench>(
    "workbench-terminal-moved",
    "keeps the graph on the terminal's worktree when its agent moves to another, and opens the detected worktree with its own graph and terminal",
    prepare,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await typeLine(browser, "claude --move side");
      await screenShows(browser, "agent --move+side at", "the agent started");
      await finishAgent(browser, fixture, "--move side");
      await browser.waitUntil(async () => (await terminal(browser))?.notes.some((note) => note.includes("The agent moved to another worktree")) === true,
        { timeout: 30000, timeoutMsg: "the terminal offers the worktree its agent moved to" });
      assert.equal(await mainTitle(browser), "Current · repo  master", "the graph stays on the terminal's own worktree");
      assert.ok(await branch(browser, "repo  side"), "the other worktree listed as its own entry");
      assert.equal(agentRuns(fixture, "claude").length, 1);
      await browser.$(TERMINAL).$("button=Open detected worktree").click();
      await currentOf(browser, "repo  side");
      await terminalOf(browser, "repo / side");
      assert.ok(!screenText(await terminal(browser)).includes("agent --move+side at"), "side's own terminal");
    },
  )),

  // UX journeys: J-RESIZE J-GRAPH-FIND J-TERMINAL; UX edges: E-TERMINAL-OPEN E-GRAPH-FIND-GO E-TERMINAL-WIDTH E-TERMINAL-COLLAPSE
  // E-TERMINAL-EXPAND E-MON-RESIZE
  scenario<{ width: number }>(
    "workbench-resize",
    "centres what the find field opens between the terminal and the Detail, resizes the terminal by its edge within room for both panels and some graph, keeps its width collapsed and in the next launch, and keeps the window and the Main area wide enough",
    async () => {
      const fixture = createWorkbench();
      return { fixture, args: ["--repo", fixture.repo], env: fixture.env, data: { width: 420 } };
    },
    async (browser, { data: { width } }) => {
      await browser.setWindowSize(1440, 900);
      await afterRender(browser);
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await browser.waitUntil(async () => (await browser.$$(".monitor .graph .svelte-flow__node").length) > 0, { timeout: 30000, timeoutMsg: "the graph drawn" });
      await camera(browser);

      // Found from the Main area, docs opens and sits between the terminal and the Detail.
      await browser.waitUntil(() => entriesFocused(browser), { timeout: 15000, timeoutMsg: "the Pane has the keyboard" });
      await press(browser, ["Enter"], () => mainFocused(browser), "the Main area focused");
      await press(browser, [primaryModifier, "f"], async () => (await findField(browser))?.focused === true, "the find field open");
      await fill(browser, FIND, "docs");
      await browser.waitUntil(async () => (await findField(browser))?.entries[0]?.name === "docs", { timeout: 15000, timeoutMsg: "docs found" });
      await press(browser, ["Enter"], async () => (await detail(browser))?.title === "Artifact docs", "docs's Detail open");
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, "a:docs");
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4;
      }, { timeout: 15000, timeoutMsg: "docs in the middle of the graph between the terminal and the Detail" });
      const between = await browser.execute((query) => {
        const node = document.querySelector('.monitor .graph .svelte-flow__node[data-id="a:docs"]')!.getBoundingClientRect();
        const panel = document.querySelector(query)!.getBoundingClientRect();
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        return panel.right < node.left + node.width / 2 && node.left + node.width / 2 < dock.left;
      }, TERMINAL);
      assert.ok(between, "docs between the terminal and the Detail");

      // The window stays at least 1280 px wide, and the Pane's divider dragged far right leaves
      // the Main area room for both panels and some graph.
      await browser.setWindowSize(1000, 800);
      await afterRender(browser);
      const narrowest = await browser.execute(() => innerWidth);
      assert.ok(narrowest >= 1280, `the window never narrower than 1280 px: ${narrowest}`);
      await browser.setWindowSize(1440, 900);
      await afterRender(browser);
      const divider = await browser.$(".monitor [data-pane-resizer]");
      const paneWidth = () => browser.execute(() => document.querySelector('section[aria-label="Pane"]')!.getBoundingClientRect().width);
      const narrow = await paneWidth();
      await browser.action("pointer", { id: "divider" }).move({ origin: divider, x: 2, y: 0 }).down().perform(true);
      try {
        await afterRender(browser);
        await browser.action("pointer", { id: "divider" }).move({ x: 1400, y: 300 }).up().perform();
      } finally { await browser.releaseActions(); }
      await browser.waitUntil(async () => (await paneWidth()) > narrow + 40, { timeout: 15000, timeoutMsg: "the Pane widened by the drag" });
      const stage = await browser.execute(() => document.querySelector(".monitor .stage")!.getBoundingClientRect().width);
      assert.ok(stage >= 760 - 1, `the Main area keeps room for both panels and some graph: ${stage}`);
      // The divider dragged back gives the Main area its room again.
      await browser.action("pointer", { id: "divider" }).move({ origin: divider, x: 2, y: 0 }).down().perform(true);
      try {
        await afterRender(browser);
        await browser.action("pointer", { id: "divider" }).move({ x: 520, y: 300 }).up().perform();
      } finally { await browser.releaseActions(); }
      await browser.waitUntil(() => browser.execute(() => document.querySelector(".monitor .stage")!.getBoundingClientRect().width > 900),
        { timeout: 15000, timeoutMsg: "the Main area wide again" });
      // The right edge resizes the terminal; hovering it shows only the resize cursor.
      const edge = () => browser.$(TERMINAL).$('[role="separator"][aria-label="Terminal width"]');
      assert.match(await browser.execute((query) => getComputedStyle(document.querySelector(`${query} [aria-label="Terminal width"]`)!).cursor, TERMINAL), /(?:ew|col)-resize/);
      await browser.action("pointer").move(await visibleCenter(browser, await edge().getElement())).perform();
      assert.equal(await browser.$('[role="tooltip"], [role="menu"]').isExisting(), false, "hovering the edge opens nothing");
      const dragEdge = async (x: number) => {
        await browser.action("pointer", { id: "terminal-edge" }).move({ origin: await edge() }).down().perform(true);
        try {
          await browser.action("pointer", { id: "terminal-edge" }).move({ x, y: 300 }).up().perform();
        } finally { await browser.releaseActions(); }
      };
      const room = await browser.execute(() => {
        const box = document.querySelector(".monitor .stage")!.getBoundingClientRect();
        return { left: box.left, right: box.right, width: box.width };
      });
      // Dragged as far right as the pointer goes: the Detail keeps a readable column beside it
      // and some graph shows between them.
      await dragEdge(Math.round(room.right - 2));
      const fit = await browser.execute((query) => {
        const panel = document.querySelector(query)!.getBoundingClientRect();
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        const stage = document.querySelector(".monitor .stage")!.getBoundingClientRect();
        const middle = document.elementFromPoint((panel.right + dock.left) / 2, stage.top + stage.height / 2);
        return { terminal: panel.width, detail: dock.width, gap: dock.left - panel.right, graph: !!middle?.closest(".graph") };
      }, TERMINAL);
      assert.ok(fit.terminal > width && fit.detail >= 320 - 1 && fit.gap >= 96 - 1 && fit.graph, `both panels and some graph fit: ${JSON.stringify(fit)}`);
      // Dragged to a chosen width, it keeps it collapsed and expanded again.
      await dragEdge(Math.round(room.left + 12 + width));
      await browser.waitUntil(async () => Math.abs(((await terminal(browser))?.box.width ?? 0) - width) <= 1, { timeout: 15000, timeoutMsg: "the dragged width" });
      await collapse(browser);
      await expand(browser);
      assert.ok(Math.abs(((await terminal(browser))?.box.width ?? 0) - width) <= 1, "expanded again at its width");

    },
    async (browser, { data: { width } }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      // Restored widths are kept within room for both panels and some graph: as wide as dragged
      // where the Main area allows it.
      const stage = await browser.execute(() => document.querySelector(".monitor .stage")!.getBoundingClientRect().width);
      const fitted = Math.max(320, Math.min(width, Math.round(stage) - 12 - 96 - 12 - 320));
      await browser.waitUntil(async () => Math.abs(((await terminal(browser))?.box.width ?? 0) - fitted) <= 1,
        { timeout: 15000, timeoutMsg: `the next launch restores the terminal's ${width}px, fitted to a ${stage}px Main area as ${fitted}px` });
    },
  ),

  // UX journey: J-TERMINAL-RESTORE; UX edges: E-TERMINAL-RESTORE-OFFER E-TERMINAL-RESTORE-RUN
  needing(["terminal-foreground"], scenario<Workbench>(
    "workbench-terminal-restore-codex",
    "offers Codex's latest session, labelled so, as `codex resume --last`, and a session its records name as `codex resume ID`, and runs the one chosen",
    prepare,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await typeLine(browser, "codex --hello");
      await finishAgent(browser, fixture, "--hello");
      await printed(browser, "agent done --hello", "Codex ran and finished");
      await endShell(browser);
      const latest = await reopen(browser);
      assert.match(latest.text, /Resume the latest Codex session: codex resume --last/);
      assert.equal(agentRuns(fixture, "codex").length, 1, "offering runs nothing");
      await browser.$(TERMINAL).$('button[aria-label="Resume the latest Codex session"]').click();
      await finishAgent(browser, fixture, "resume --last");
      await printed(browser, "agent done resume+--last", "Codex resumed its latest session");
      const [first, resumed] = agentRuns(fixture, "codex");
      assert.deepEqual(resumed, { args: "resume --last", cwd: first.cwd }, "in Codex's directory");

      // A session its own records name for this directory resumes by its ID.
      await typeLine(browser, `codex resume ${fixture.session}`);
      await finishAgent(browser, fixture, `resume ${fixture.session}`);
      await printed(browser, `agent done resume+${fixture.session}`, "Codex ran its session and finished");
      await endShell(browser);
      const exact = await reopen(browser);
      assert.match(exact.text, new RegExp(`Resume the Codex session: codex resume ${fixture.session}`));
      const runs = agentRuns(fixture, "codex").length;
      await browser.$(TERMINAL).$('button[aria-label="Resume the Codex session"]').click();
      await browser.waitUntil(() => agentRuns(fixture, "codex").length === runs + 1, { timeout: 30000, timeoutMsg: "the session resumed" });
      assert.deepEqual(agentRuns(fixture, "codex").at(-1), { args: `resume ${fixture.session}`, cwd: first.cwd });
    },
  )),

  // UX journey: J-WORKTREE-REMOVE; UX edges: E-WORKTREE-REMOVE-ASK E-WORKTREE-REMOVE-CANCEL E-WORKTREE-REMOVE E-TERMINAL-REMOVED
  scenario<Holding>(
    "workbench-worktree-remove",
    "asks before removing a worktree, keeps everything on Cancel or Esc, says Git's refusal for the main checkout, a locked and a dirty worktree, and otherwise removes the folder and ends its terminal, deleting a generated branch only while unrenamed and without commits of its own",
    prepareHolding,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      // It asks first, saying what goes; Cancel and Esc keep everything.
      const asked = await askRemoval(browser);
      assert.ok(asked.includes("is removed with its folder") && asked.includes("its terminal ends with everything running in it"), asked);
      assert.ok(asked.includes("deleted too while nobody renamed it and it has no commits of its own"), asked);
      await browser.$('[role="dialog"]').$("button=Cancel").click();
      await browser.waitUntil(async () => (await removal(browser)) === null, { timeout: 15000, timeoutMsg: "Cancel closed the dialog" });
      await askRemoval(browser);
      await press(browser, ["Escape"], async () => (await removal(browser)) === null, "Esc closes the dialog");
      assert.ok(exists(fixture.repo) && await branch(browser, "repo  master"), "nothing removed");

      // Git refuses the main checkout, a locked worktree and one with uncommitted work.
      await removalRefused(browser, "the main checkout refused");
      assert.ok(exists(fixture.repo) && await branch(browser, "repo  master"), "the main checkout stays");
      git(fixture.repo, "-C", fixture.repo, "worktree", "lock", fixture.side);
      await selectBranch(browser, "repo  side");
      await terminalOf(browser, "repo / side");
      // The lock stays until the locked worktree's own refusal shows.
      await removalRefused(browser, "the locked worktree refused");
      git(fixture.repo, "-C", fixture.repo, "worktree", "unlock", fixture.side);
      writeText(nativePath(fixture.side, "docs", "notes.md"), "Unfinished work.\n");
      await selectBranch(browser, "repo  master");
      await selectBranch(browser, "repo  side");
      await removalRefused(browser, "the dirty worktree refused");
      assert.ok(exists(fixture.side) && await branch(browser, "repo  side"), "the refused worktree stays");

      // A clean worktree goes with its folder, and its terminal ends with the program running in
      // it, which holds a connection open until it ends; its own branch stays.
      git(fixture.repo, "-C", fixture.side, "checkout", "--", ".");
      await typeLine(browser, `claude --hold ${fixture.held.port}`);
      await held(browser, fixture, { opened: 1, ended: 0 }, "the program in side's terminal runs");
      await askRemoval(browser);
      await confirmRemoval(browser);
      await held(browser, fixture, { opened: 1, ended: 1 }, "the program in the removed worktree's terminal ended");
      await browser.waitUntil(async () => (await terminal(browser))?.notes.some((note) => note.startsWith("The terminal ended")) === true,
        { timeout: 30000, timeoutMsg: "the removed worktree's terminal ended" });
      await browser.waitUntil(async () => (await branch(browser, "repo  side")) === undefined, { timeout: 30000, timeoutMsg: "gone from the Pane" });
      assert.ok(!exists(fixture.side), "its folder removed");
      assert.ok(branchNames(fixture).includes("side"), "a branch artifactize did not generate stays");

      /** A new worktree from `commit`, opened: its generated branch and folder. */
      let last = "";
      const make = async (commit: string) => {
        await selectBranch(browser, "repo  master");
        await newWorktreeFrom(browser, commit);
        const made = await created(browser, last);
        last = made.branch;
        await currentOf(browser, `repo  ${made.branch}`);
        await terminalOf(browser, `repo / ${made.branch}`);
        return made;
      };
      // Generated, never renamed, every commit on another branch: the branch goes too.
      const plain = await make("Check the docs twice");
      await askRemoval(browser);
      await confirmRemoval(browser);
      await browser.waitUntil(() => !exists(asNativePath(plain.folder)), { timeout: 30000, timeoutMsg: "the generated worktree's folder removed" });
      await browser.waitUntil(() => !branchNames(fixture).includes(plain.branch), { timeout: 15000, timeoutMsg: "its generated branch deleted" });
      // With a commit of its own, the generated branch stays.
      const worked = await make("Fixture inputs");
      git(fixture.repo, "-C", asNativePath(worked.folder), "commit", "--allow-empty", "-qm", "Own work");
      await askRemoval(browser);
      await confirmRemoval(browser);
      await browser.waitUntil(() => !exists(asNativePath(worked.folder)), { timeout: 30000, timeoutMsg: "the worked-on worktree's folder removed" });
      assert.ok(branchNames(fixture).includes(worked.branch), "a generated branch with its own commit stays");
      // Renamed, the generated branch stays.
      const renamed = await make("Fixture inputs");
      git(fixture.repo, "-C", asNativePath(renamed.folder), "branch", "-m", renamed.branch, "kept-name");
      await listed(browser, "repo  kept-name", 30000);
      await askRemoval(browser);
      await confirmRemoval(browser);
      await browser.waitUntil(() => !exists(asNativePath(renamed.folder)), { timeout: 30000, timeoutMsg: "the renamed worktree's folder removed" });
      assert.ok(branchNames(fixture).includes("kept-name"), "a renamed generated branch stays");
    },
  ),

  // UX journey: J-TERMINAL-DETACH; UX edges: E-TERMINAL-NOT-PERSISTENT E-QUIT-CLEAN
  needing(["breakaway-job"], scenario<Holding>(
    "workbench-terminal-not-persistent",
    "says a worktree's terminal ends when the app closes when the system keeps it in the Job the app runs in, and quits at once",
    prepareHolding,
    async (browser, { data: fixture }) => {
      await currentOf(browser, "repo  master");
      await terminalOf(browser, "repo / master");
      await browser.waitUntil(async () => (await terminal(browser))?.notes.includes("Not persistent. This terminal ends when Artifactize closes.") === true,
        { timeout: 30000, timeoutMsg: "the panel says the terminal ends with the app" });
      await typeLine(browser, "echo in-the-job");
      await printed(browser, "in-the-job", "a shell runs in the worktree");
      // A program running in it holds a connection open until it ends; closing the app ends it.
      await typeLine(browser, `claude --hold ${fixture.held.port}`);
      await held(browser, fixture, { opened: 1, ended: 0 }, "the program in the terminal runs");
      await selectBranch(browser, "repo  master");
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('section[aria-label="Pane"]')),
        { timeout: 15000, timeoutMsg: "the keyboard in the Pane" });
      assert.deepEqual(heldConnections(fixture.held.record), { opened: 1, ended: 0 }, "the program runs until the app closes");
      await sessionEndedByApp(browser, () => browser.keys(["q"]));
      await held(browser, fixture, { opened: 1, ended: 1 }, "closing the app ended the program in its terminal");
    },
  )),
];
