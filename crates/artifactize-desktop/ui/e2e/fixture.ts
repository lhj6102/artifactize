// Fixture state for the end-to-end tests, built only through the real `artifactize` CLI:
// a release Artifact with a waiting Human sign-off, its tools and owner schemas, and a
// dependent Artifact that waits for it.
// Portable domain inputs only: WDIO's outer runner/service owns the native app lifecycle.
import { readCandidate } from "./build.ts";
import { settleRuns } from "./runs.ts";
import { environment, git } from "./environment.ts";
import { prepareFixture, releaseFixtures } from "./build-fixtures.ts";
import { commandFields, runtimeProfile, type Operation } from "./native-fixtures.ts";
import { waitingRun } from "./run-fixtures.ts";
import {
  exists, makeDirectory, nativePath, parentOf, portablePath, runProgram, runProgramAsync, textPath, updateDatabase, writeText,
  type Environment, type NativePath,
} from "./platform.ts";
export { environment } from "./environment.ts";
export { createdRoots, releaseFixtures, settleFixtures } from "./build-fixtures.ts";
export { slowTool } from "./process-fixtures.ts";

/** The declared immutable candidates inherited from the owning suite or tool session. */
export const cli = () => readCandidate().cli;
export const app = () => readCandidate().app;
/** Roots remain available until the outer owner releases them after app/service exit. */

export type Fixture = { root: NativePath; repo: NativePath; state: NativePath };

/** The fixture's declared launch-tool stand-in actually wrote its completion marker. */
export function launched(fixture: Fixture): boolean {
  return exists(nativePath(fixture.root, "launched"));
}

/** Let the fixture's `talk` stand-in go on: it prints its last lines and exits. */
export function finishTalk(fixture: Fixture): void {
  writeText(nativePath(fixture.root, "talk.gate"), "go");
}

/** Declared Human content, captured in the request before the app starts. */
export type SignoffDefinition = { title: string; instruction: string; scalars?: boolean; report?: boolean };
const DEFAULT_SIGNOFF: SignoffDefinition = {
  title: "A person approves the release",
  instruction: "Read the release notes and approve them.",
};

const release = (root: NativePath, signoff: SignoffDefinition = DEFAULT_SIGNOFF) => `name = "release"
tags = ["type:release"]
fingerprint = { files = ["."] }

[views.human_tools]
${signoff.report ? `report = { description = "Print long lines on both output streams.", kind = "output", ${commandFields("report")} }` : ""}
notes = { description = "Print the release notes of {artifactName}.", kind = "output", ${commandFields("read", textPath("{artifactPath}", "notes.md"))} }
open = { description = "Open {artifactName}.", kind = "launch", ${commandFields("launch", portablePath(nativePath(root, "launched")))} }
fail = { description = "Fail on purpose.", kind = "output", ${commandFields("fail")} }
talk = { description = "Open a stand-in that prints what the reviewer chose.", kind = "launch", ${commandFields("talk", portablePath(nativePath(root, "talk.gate")))} }
slow = { description = "Wait until cancelled; serve the owned lifetime endpoint.", kind = "output", ${commandFields("slow", portablePath(nativePath(root, "slow.endpoint")))} }

[evals.signoff]
title = ${JSON.stringify(signoff.title)}
profile = { kind = "human" }

[evals.signoff.payload]
instruction = ${JSON.stringify(signoff.instruction)}

[evals.signoff.pass_schema]
type = "object"
properties.approved = { const = true }
properties.comment = { type = "string", minLength = 1, description = "What you checked." }
${signoff.scalars ? `properties.score = { type = "number" }
properties.note = { type = "string" }
properties.confirmed = { type = "boolean" }
properties.channel = { enum = ["stable", "preview"] }` : ""}
required = ["approved", "comment"]
additionalProperties = false

[evals.signoff.fail_schema]
type = "object"
properties.reason = { type = "string", minLength = 3 }
properties.findings = { type = "array", items = { type = "object", properties = { problem = { type = "string", minLength = 1 }, severity = { enum = ["low", "high"] } }, required = ["problem"], additionalProperties = false } }
properties.steps = { type = "array", items = { type = "string" } }
required = ["reason"]
additionalProperties = false
`;

const SHIP = `name = "ship"

[evals.check]
title = "Ship the release"
profile = ${runtimeProfile()}

[evals.check.payload]
instruction = """
Ship {release}."""
`;

const DOCS = `name = "docs"

[views.human_tools]
reference = { description = "Print documentation review notes.", builtin = "read", args = ["{artifactPath}/notes.md"] }

[evals.lint]
title = "Docs build"
profile = ${runtimeProfile()}

[evals.lint.payload]
instruction = """
Build the docs."""
`;

/** Run the CLI against the fixture; returns its exit status. */
export function artifactize(fixture: Fixture, ...args: string[]): number {
  return runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, ...args], { env: environment(fixture) }).status;
}

export type Saved = {
  id: string;
  runId: string;
  evalId: string;
  status: string;
  claim: { reviewer: string } | null;
  result: Record<string, unknown> | null;
};

/** Remote status without blocking an external store served by the calling test. */
export async function remoteStatus(fixture: Fixture): Promise<{ code: number; stdout: string }> {
  const { status, stdout } = await runProgramAsync(cli(), ["--state-dir", fixture.state, "--repo", fixture.repo, "remote", "status", "--json"],
    { env: { ...environment(fixture), ARTIFACTIZE_REMOTE: "" } });
  return { code: status, stdout };
}

/** The providers this machine has set up, as `provider list --json` prints them with the app's `env`. */
export async function providerList(fixture: Fixture, env: Environment = {}): Promise<{ code: number; stdout: string }> {
  const { status, stdout } = await runProgramAsync(cli(), ["--state-dir", fixture.state, "--repo", fixture.repo, "provider", "list", "--json"],
    { env: { ...environment(fixture), ...env } });
  return { code: status, stdout };
}

/** `verify --all --wait --json` without blocking a provider served by the calling test. */
export async function verifyAll(fixture: Fixture): Promise<{ code: number; stdout: string }> {
  const { status, stdout } = await runProgramAsync(cli(), ["--state-dir", fixture.state, "--repo", fixture.repo, "verify", "--all", "--wait", "--json"],
    { env: environment(fixture) });
  return { code: status, stdout };
}

/** The saved requests, as `request list --json` prints them. */
export function requests(fixture: Fixture): Saved[] {
  const result = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, "request", "list", "--json"],
    { env: environment(fixture) });
  if (result.status !== 0) throw new Error(result.stderr);
  return JSON.parse(result.stdout) as Saved[];
}

/** All saved Runs through the public CLI, including non-default statuses and old entries.
 * An empty state database is allowed: absence of Runs is a semantic query, not a file test. */
export function savedRuns(fixture: Fixture): { id: string }[] {
  const result = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "list", "--all", "--limit", "4294967295", "--json"],
    { env: environment(fixture) });
  if (result.status !== 0) throw new Error(result.stderr);
  return JSON.parse(result.stdout) as { id: string }[];
}

/** Explicit domain cancellation settles saved state and joins its published workers. */
export function cancelRuns(fixture: Fixture): void {
  settleRuns(cli(), fixture.repo, fixture.state);
}

/** The saved Human sign-off request. */
export function signoff(fixture: Fixture): Saved {
  const found = requests(fixture).find((request) => request.evalId === "release/signoff");
  if (!found) throw new Error("no release/signoff request");
  return found;
}

/** Declare the release, ship and docs Artifacts in a new Git repository at `repo`. */
function declare(repo: NativePath, notes = "# Release notes\n- Ready to ship.\n", signoff: SignoffDefinition = DEFAULT_SIGNOFF) {
  for (const [name, text] of [
    ["release", release(parentOf(repo), signoff)],
    ["ship", SHIP],
    ["docs", DOCS],
  ]) {
    writeText(nativePath(makeDirectory(nativePath(repo, name)), "index.artf"), text);
  }
  writeText(nativePath(repo, "release", "notes.md"), notes);
  writeText(nativePath(repo, "docs", "notes.md"), "Documentation checked.\n");
  git(repo, "init", "-q", "--initial-branch=master", repo);
}

/** Submit every eval and observe its saved scheduler Human boundary. */
export function verifyWaiting(fixture: Fixture): Promise<string> {
  return waitingRun(cli(), fixture);
}

/** Current declarations only: no verify and no saved Run. */
export function createGraph(): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    declare(repo);
    return { root, repo, state: nativePath(root, "state") };
  });
}

/** A fresh repository and state with one Run waiting for the Human sign-off. */
export async function create(signoff: SignoffDefinition = DEFAULT_SIGNOFF): Promise<Fixture> {
  return prepareFixture(async (root) => {
    const repo = nativePath(root, "repo");
    const state = nativePath(root, "state");
    declare(repo, undefined, signoff);
    const fixture = { root, repo, state };
    await verifyWaiting(fixture);
    return fixture;
  });
}

/**
 * Another repository `name` beside the fixture's, in the same state, with the same eval IDs
 * and its own waiting sign-off (its notes differ, so it joins no other); returns its path.
 */
export async function addRepository(fixture: Fixture, name: string, signoff: SignoffDefinition = DEFAULT_SIGNOFF): Promise<NativePath> {
  const repo = nativePath(fixture.root, name);
  declare(repo, `# Release notes of ${name}\n- Ready to ship.\n`, signoff);
  await verifyWaiting({ ...fixture, repo });
  return repo;
}

function humanExample(repo: NativePath, name: string, criterion: string, title: string, tags: string[] = []): void {
  const directory = makeDirectory(nativePath(repo, name));
  writeText(nativePath(directory, "index.artf"), `name = ${JSON.stringify(name)}
tags = ${JSON.stringify(tags)}

[evals.${criterion}]
title = ${JSON.stringify(title)}
profile = { kind = "human" }

[evals.${criterion}.payload]
instruction = ${JSON.stringify(`Review ${title}.`)}
`);
}

function deferredHuman(fixture: Fixture): string {
  const submitted = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state,
    "verify", "--all", "--defer-human", "--wait", "--json"], { env: environment(fixture) });
  if (submitted.status !== 5) throw new Error(submitted.stderr || submitted.stdout || "Human deferral did not complete");
  const saved = JSON.parse(submitted.stdout) as { status: string; requests: Saved[] };
  const run = saved.requests[0]?.runId;
  if (saved.status !== "SIGNOFFS_PENDING" || !run || saved.requests.some((request) => request.runId !== run || request.status !== "DEFERRED")) {
    throw new Error(`unexpected deferred Human result: ${submitted.stdout}`);
  }
  settleRuns(cli(), fixture.repo, fixture.state);
  return run;
}

/** Historical tree identity belongs to a Run, not today's declarations. Every Run
 * is terminal and its writer joined before replacement or controlled chronology. */
export async function createNavigationFixture(): Promise<{
  fixture: Fixture; scopes: { repo: string[]; other: string[] }; runs: { id: string; eval: string }[];
}> {
  return prepareFixture(async (root) => {
    const fixture = { root, repo: nativePath(root, "repo"), state: nativePath(root, "state") };
    humanExample(fixture.repo, "subject", "first-criterion", "First navigation criterion");
    git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
    const first = deferredHuman(fixture);
    humanExample(fixture.repo, "subject", "second-criterion", "Second navigation criterion");
    const second = deferredHuman(fixture);
    const other = { ...fixture, repo: nativePath(root, "other") };
    humanExample(other.repo, "elsewhere", "other-criterion", "Other scope criterion");
    git(other.repo, "init", "-q", "--initial-branch=master", other.repo);
    const third = deferredHuman(other);
    // All writers are joined. These explicit fixture times determine newest-first
    // navigation independently of wall-clock resolution and scheduling.
    const saved = requests(fixture);
    updateDatabase(nativePath(fixture.state, "state.sqlite"), (db) => {
      for (const [id, created] of [[first, "2000-01-01T00:00:00.000000000Z"],
        [second, "2000-01-02T00:00:00.000000000Z"], [third, "2000-01-03T00:00:00.000000000Z"]]) {
        db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, id);
        for (const request of saved.filter((request) => request.runId === id)) {
          db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, request.id);
        }
      }
    });
    return { fixture, scopes: { repo: [first, second], other: [third] }, runs: [
      { id: first, eval: "first-criterion" }, { id: second, eval: "second-criterion" }, { id: third, eval: "other-criterion" },
    ] };
  });
}

/** Real worktrees of the fixture's one Git repository, each with its own identified Run. */
export async function addWorktrees(fixture: Fixture): Promise<string[]> {
  git(fixture.repo, "-C", fixture.repo, "add", ".");
  git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Fixture inputs");
  const runs: string[] = [];
  for (const name of ["scope-one", "scope-two"]) {
    const repo = nativePath(fixture.root, name);
    git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", name, repo);
    writeText(nativePath(repo, "release", "notes.md"), `# Notes of ${name}\n- Distinct consumer inputs.\n`);
    runs.push(await verifyWaiting({ ...fixture, repo }));
  }
  return runs;
}

/**
 * A fresh repository whose `count` Artifacts n000 … form one chain, each referring to the one
 * before it, all verified GREEN: a Run too large for the graph to draw every node at once.
 */
export function createChain(count: number): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    const state = nativePath(root, "state");
    const name = (index: number) => `n${String(index).padStart(3, "0")}`;
    for (let index = 0; index < count; index += 1) {
      const reference = index === 0 ? "the start" : `{${name(index - 1)}}`;
      const text = `name = "${name(index)}"

  [evals.check]
  title = "Check ${name(index)}"
  profile = ${runtimeProfile()}

  [evals.check.payload]
  instruction = """
  Check ${reference}."""
  `;
      writeText(nativePath(makeDirectory(nativePath(repo, name(index))), "index.artf"), text);
    }
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
    if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
    return fixture;
  });
}

/**
 * Give the saved Runs `runs` and their requests explicit creation times, oldest first, a minute
 * apart and ending a minute ago: their order never depends on the clock's resolution or on how
 * fast the writers ran. A Run still waiting keeps them: its later writes read the record first.
 */
export function orderRuns(fixture: Fixture, runs: readonly string[]): void {
  const saved = requests(fixture);
  const now = Date.now();
  updateDatabase(nativePath(fixture.state, "state.sqlite"), (db) => {
    runs.forEach((id, index) => {
      // The saved form: nanoseconds, UTC.
      const created = new Date(now - (runs.length - index) * 60_000).toISOString().replace(/\.(\d{3})Z$/, ".$1000000Z");
      if (db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, id).changes !== 1) {
        throw new Error(`Run ${id} was not dated`);
      }
      for (const request of saved.filter((request) => request.runId === id)) {
        db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, request.id);
      }
    });
  });
}

/**
 * A one-Artifact repository verified GREEN `count` times in a row, with no Run in progress: a
 * branch whose chip row holds more Runs than it first shows. Returns the Runs, oldest first.
 */
export function createHistory(count: number): { fixture: Fixture; runs: string[] } {
  const fixture = createChain(1);
  return prepareFixture(() => {
    const runs = savedRuns(fixture).map((run) => run.id);
    while (runs.length < count) {
      const completed = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--wait", "--json"],
        { env: environment(fixture) });
      if (completed.status !== 0) throw new Error(completed.stderr || completed.stdout);
      runs.push((JSON.parse(completed.stdout) as { id: string }).id);
    }
    // Join every writer before the app reads the Runs, so none of them is still in progress.
    settleRuns(cli(), fixture.repo, fixture.state);
    orderRuns(fixture, runs);
    return { fixture, runs };
  }, fixture.root);
}

/**
 * One branch `shared` verified from two folders by two callers: first in the worktree `first`
 * by a person, then, once that worktree was removed, in the worktree `second` by an agent
 * session (its retry-safe attempt key, as an agent's `verify_start` saves it). With the
 * repository's own Run on `master` before them; returns the Runs, oldest first.
 */
export function createSharedBranch(): { fixture: Fixture; master: string; shared: string[] } {
  const fixture = createChain(1);
  return prepareFixture(() => {
    const [master] = savedRuns(fixture).map((run) => run.id);
    git(fixture.repo, "-C", fixture.repo, "add", ".");
    git(fixture.repo, "-C", fixture.repo, "commit", "-qm", "Fixture inputs");
    const verify = (repo: NativePath, ...args: string[]) => {
      const completed = runProgram(cli(), ["--repo", repo, "--state-dir", fixture.state, "verify", "--all", "--wait", "--json", ...args],
        { env: environment(fixture) });
      if (completed.status !== 0) throw new Error(completed.stderr || completed.stdout);
      return (JSON.parse(completed.stdout) as { id: string }).id;
    };
    const first = nativePath(fixture.root, "first");
    git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", "shared", first);
    const byPerson = verify(first);
    settleRuns(cli(), first, fixture.state);
    git(fixture.repo, "-C", fixture.repo, "worktree", "remove", "--force", first);
    const second = nativePath(fixture.root, "second");
    git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", second, "shared");
    const byAgent = verify(second, "--request-key", "agent-session.e2e-1");
    settleRuns(cli(), second, fixture.state);
    orderRuns(fixture, [master, byPerson, byAgent]);
    return { fixture, master, shared: [byPerson, byAgent] };
  }, fixture.root);
}

/** A folder `name` outside Git beside the fixture's repository, in the same state, that is itself
 * an Artifact `plain` verified GREEN; returns its path and its Run. */
export function addFolder(fixture: Fixture, name: string): { folder: NativePath; run: string } {
  const folder = nativePath(fixture.root, name);
  writeText(nativePath(makeDirectory(folder), "index.artf"), `name = "plain"

[evals.check]
title = "Check plain"
profile = ${runtimeProfile()}

[evals.check.payload]
instruction = """
Check the plain folder."""
`);
  const completed = runProgram(cli(), ["--repo", folder, "--state-dir", fixture.state, "verify", "--all", "--wait", "--json"], { env: environment(fixture) });
  // Its Run ended with the wait; settling here would also cancel the state's waiting Runs.
  if (completed.status !== 0) throw new Error(completed.stderr || completed.stdout);
  return { folder, run: (JSON.parse(completed.stdout) as { id: string }).id };
}

/** Change release's notes: its sign-off's inputs change, so the next verify asks a person again. */
export function changeNotes(fixture: Fixture, text: string): void {
  writeText(nativePath(fixture.repo, "release", "notes.md"), text);
}

/** Sign the waiting request `id` off GREEN with `fields` as bob through the CLI, as another process. */
export function signOff(fixture: Fixture, id: string, fields = '{"approved":true,"comment":"ok"}', reviewer = "bob"): void {
  for (const args of [["request", "claim", id, "--reviewer", reviewer],
    ["request", "submit", id, "--reviewer", reviewer, "--verdict", "GREEN", "--fields", fields]]) {
    if (artifactize(fixture, ...args) !== 0) throw new Error(`cannot sign off ${id}: ${args.join(" ")}`);
  }
}

/** The status Run `run` ends with, observed by the CLI once it has ended. */
export function ended(fixture: Fixture, run: string): string {
  const shown = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", run, "--wait", "--json"], { env: environment(fixture) });
  if (![0, 1, 2, 4, 5].includes(shown.status)) throw new Error(shown.stderr || `cannot observe Run ${run}`);
  return (JSON.parse(shown.stdout) as { status: string }).status;
}

/** A repository `name` beside the fixture's, in the same state, whose `count` Artifacts each
 * wait for a person's sign-off; returns their requests. */
export async function addWaiting(fixture: Fixture, name: string, count: number): Promise<string[]> {
  const repo = nativePath(fixture.root, name);
  for (let index = 0; index < count; index += 1) {
    humanExample(repo, `gate${String(index).padStart(2, "0")}`, "signoff", `Gate ${index} approval`);
  }
  git(repo, "init", "-q", "--initial-branch=master", repo);
  const target = { ...fixture, repo };
  const run = await verifyWaiting(target);
  return requests(target).filter((request) => request.runId === run).map((request) => request.id);
}

/** A saved Run's selection as written, and as a version that no longer reads it would see it. */
const [readableSelection, unreadableSelection] = ['"selection":{"kind":"all"}', '"selection":{"kind":"dependency"}'];

/**
 * One GREEN Artifact `n000` verified four times. The three older Runs are rewritten as Runs saved
 * with a selection kind this version does not read, so the list of Runs cannot read them; the
 * newest stays readable. `restore` writes them back as they were saved, so teardown settles every
 * Run through the public CLI.
 */
export function createUnreadableRuns(allUnreadable = false): { fixture: Fixture; readable: string; unreadable: string[]; restore: () => void } {
  const fixture = createChain(1);
  return prepareFixture(() => {
    const runs = savedRuns(fixture).map((run) => run.id);
    for (let index = 0; index < 3; index += 1) {
      const completed = runProgram(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--wait", "--json"],
        { env: environment(fixture) });
      if (completed.status !== 0) throw new Error(completed.stderr || completed.stdout);
      runs.push((JSON.parse(completed.stdout) as { id: string }).id);
    }
    // Join every writer before rewriting saved Runs, so finalization cannot overwrite them.
    settleRuns(cli(), fixture.repo, fixture.state);
    if (runs.length !== 4) throw new Error(`expected four fixture Runs, saved ${runs.join(", ")}`);
    const saved = requests(fixture);
    const unreadable = allUnreadable ? runs : runs.slice(0, 3);
    const database = nativePath(fixture.state, "state.sqlite");
    const rewrite = (from: string, to: string) => updateDatabase(database, (db) => {
      for (const id of unreadable) {
        const changed = db.prepare("UPDATE runs SET data=replace(data,?1,?2) WHERE id=?3 AND instr(data,?1)>0").run(from, to, id);
        if (changed.changes !== 1) throw new Error(`Run ${id} was not rewritten`);
      }
    });
    updateDatabase(database, (db) => {
      // Explicit times make the readable Run the newest, independently of clock resolution.
      runs.forEach((id, index) => {
        const created = `2000-01-0${index + 1}T00:00:00.000000000Z`;
        db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, id);
        for (const request of saved.filter((request) => request.runId === id)) {
          db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, request.id);
        }
      });
    });
    rewrite(readableSelection, unreadableSelection);
    return {
      fixture, readable: runs[3], unreadable,
      restore: () => {
        rewrite(unreadableSelection, readableSelection);
        // The readable Run too, if a case spoiled it.
        if (!allUnreadable) updateDatabase(database, (db) => db.prepare("UPDATE runs SET data=replace(data,?1,?2) WHERE id=?3").run(unreadableSelection, readableSelection, runs[3]));
      },
    };
  }, fixture.root);
}

/** Make `run` of a fixture from {@link createUnreadableRuns} unreadable too, as a write by
 * another version would; that fixture's restore undoes it. */
export function spoilRun(fixture: Fixture, run: string): void {
  updateDatabase(nativePath(fixture.state, "state.sqlite"), (db) => {
    if (db.prepare("UPDATE runs SET data=replace(data,?1,?2) WHERE id=?3 AND instr(data,?1)>0").run(readableSelection, unreadableSelection, run).changes !== 1) {
      throw new Error(`Run ${run} was not rewritten`);
    }
  });
}

/**
 * A fresh repository with one Artifact in each state of a Run that is not simply GREEN:
 * `red` fails, `crash` exceeds its runtime deadline (ERROR), `down` is blocked by `red`, `after` by
 * `crash`, `fine` is GREEN, and `later` waits for the Human sign-off of `gate`. Three of them have no dependency, so they share the graph's first column.
 */
export async function createStates(): Promise<Fixture> {
  return prepareFixture(async (root) => {
    const repo = nativePath(root, "repo");
    const state = nativePath(root, "state");
    const artifact = (name: string, operation: Operation, instruction: string, timeoutMs?: number) => {
      writeText(
        nativePath(makeDirectory(nativePath(repo, name)), "index.artf"),
        `name = "${name}"

  [evals.check]
  title = "Check ${name}"
  profile = ${runtimeProfile(operation, timeoutMs)}

  [evals.check.payload]
  instruction = """
  ${instruction}"""
  `,
      );
    };
    artifact("red", "fail", "Fail.");
    // Every ordinary nonzero exit is RED. A real product timeout is an operational
    // ERROR on all platforms, unlike self-signalling a Node process on Windows.
    const errorDeadlineMs = 1000;
    artifact("crash", "hold", "Exceed the declared runtime deadline.", errorDeadlineMs);
    artifact("down", "pass", "Run after {red}.");
    artifact("fine", "pass", "Pass.");
    artifact("after", "pass", "Run after {crash}.");
    // A sign-off nobody has given yet, and what waits for it.
    writeText(
      nativePath(makeDirectory(nativePath(repo, "gate")), "index.artf"),
      `name = "gate"

  [evals.signoff]
  title = "A person approves"
  profile = { kind = "human" }

  [evals.signoff.payload]
  instruction = """
  Approve."""
  `,
    );
    artifact("later", "pass", "Run after {gate}.");
    // What a blocked and a waiting Artifact pass on along their own relations.
    artifact("past", "pass", "Run after {down}.");
    artifact("beyond", "pass", "Run after {later}.");
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    // RED/ERROR are committed while the independent Human sign-off keeps the Run waiting.
    await verifyWaiting(fixture);
    return fixture;
  });
}

/**
 * A fresh repository whose one Artifact `app` has an eval for each of `names`, all verified
 * GREEN: an Artifact whose Detail has a tab for each of several evals.
 */
export function createEvals(names: string[]): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    const state = nativePath(root, "state");
    const evals = names
      .map(
        (name) => `
  [evals.${name}]
  title = "Check ${name}"
  profile = ${runtimeProfile()}

  [evals.${name}.payload]
  instruction = """
  Check ${name}."""
  `,
      )
      .join("");
    writeText(nativePath(makeDirectory(nativePath(repo, "app")), "index.artf"), `name = "app"\n${evals}`);
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
    if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
    return fixture;
  });
}

/**
 * A repository verified once, its Run ended RED: `red` fails, `down` runs after it and so is
 * blocked, and `app`'s three evals passed. Afterwards two of `app`'s instructions changed, so
 * a Current would run those two and reuse the third.
 */
export function createMixed(): Fixture {
  return prepareFixture((root) => {
    const repo = nativePath(root, "repo");
    const state = nativePath(root, "state");
    const check = (name: string, operation: Operation, instruction: string, title = `Check ${name}`) => `
[evals.${name}]
title = "${title}"
profile = ${runtimeProfile(operation)}

[evals.${name}.payload]
instruction = """
${instruction}"""
`;
    writeText(nativePath(makeDirectory(nativePath(repo, "red")), "index.artf"), `name = "red"\n${check("check", "fail", "Fail.")}`);
    writeText(nativePath(makeDirectory(nativePath(repo, "down")), "index.artf"), `name = "down"\n${check("check", "pass", "Run after {red}.")}`);
    // alpha's title is wider than the Detail: its row wraps the title, never its status.
    const title = "Alpha follows every rule of the frontend checklist the desktop UI keeps, from its state owners to its effects";
    const app = (changed: boolean) => `name = "app"\n${check("alpha", "pass", "Check alpha.", title)}${
      check("beta", "pass", changed ? "Check beta again." : "Check beta.")}${check("gamma", "pass", changed ? "Check gamma again." : "Check gamma.")}`;
    writeText(nativePath(makeDirectory(nativePath(repo, "app")), "index.artf"), app(false));
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
    if (status !== 1) throw new Error(`verify exited ${status}, expected 1 (RED)`);
    settleRuns(cli(), repo, state);
    writeText(nativePath(repo, "app", "index.artf"), app(true));
    return fixture;
  });
}

/**
 * An empty repository and state outside any Git work tree (the system temporary directory,
 * or TMPDIR): token storage refuses a state inside a repository, as the CLI does. `remove`
 * deletes it.
 */
export function createOutside(): Fixture & { remove: () => Promise<void> } {
  return prepareFixture((root) => {
    const repo = makeDirectory(nativePath(root, "repo"));
    // The caller invokes this only after the service/app has exited.
    const remove = () => releaseFixtures([root]);
    return { root, repo, state: nativePath(root, "state"), remove };
  });
}
