// Fixture state for the end-to-end tests, built only through the real `artifactize` CLI:
// a release Artifact with a waiting Human sign-off, its tools and owner schemas, and a
// dependent Artifact that waits for it.
// Portable domain inputs only: WDIO's outer runner/service owns the native app lifecycle.
import { execFile, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { readCandidate } from "./build.ts";
import { settleRuns } from "./runs.ts";
import { environment, git } from "./environment.ts";
import { prepareFixture, releaseFixtures } from "./build-fixtures.ts";
import { commandFields, runtimeProfile, type Operation } from "./native-fixtures.ts";
import { waitingRun } from "./run-fixtures.ts";
export { environment } from "./environment.ts";
export { createdRoots, releaseFixtures } from "./build-fixtures.ts";
export { slowTool } from "./process-fixtures.ts";

/** The declared immutable candidates inherited from the owning suite or tool session. */
export const cli = () => readCandidate().cli;
export const app = () => readCandidate().app;
/** Roots remain available until the outer owner releases them after app/service exit. */

export type Fixture = { root: string; repo: string; state: string };

/** The fixture's declared launch-tool stand-in actually wrote its completion marker. */
export function launched(fixture: Fixture): boolean {
  return existsSync(join(fixture.root, "launched"));
}

/** Declared Human content, captured in the request before the app starts. */
export type SignoffDefinition = { title: string; instruction: string };
const DEFAULT_SIGNOFF: SignoffDefinition = {
  title: "A person approves the release",
  instruction: "Read the release notes and approve them.",
};

const release = (root: string, signoff: SignoffDefinition = DEFAULT_SIGNOFF) => `name = "release"
tags = ["type:release"]
fingerprint = { files = ["."] }

[views.human_tools]
notes = { description = "Print the release notes of {artifactName}.", kind = "output", ${commandFields("read", join("{artifactPath}", "notes.md"))} }
open = { description = "Open {artifactName}.", kind = "launch", ${commandFields("launch", join(root, "launched"))} }
fail = { description = "Fail on purpose.", kind = "output", ${commandFields("fail")} }
slow = { description = "Wait until cancelled; serve the owned lifetime endpoint.", kind = "output", ${commandFields("slow", join(root, "slow.endpoint"))} }

[evals.signoff]
title = ${JSON.stringify(signoff.title)}
profile = { kind = "human" }

[evals.signoff.payload]
instruction = ${JSON.stringify(signoff.instruction)}

[evals.signoff.pass_schema]
type = "object"
properties.approved = { const = true }
properties.comment = { type = "string", description = "What you checked." }
required = ["approved", "comment"]
additionalProperties = false

[evals.signoff.fail_schema]
type = "object"
properties.reason = { type = "string", minLength = 3 }
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

[evals.lint]
title = "Docs build"
profile = ${runtimeProfile()}

[evals.lint.payload]
instruction = """
Build the docs."""
`;

/** Run the CLI against the fixture; returns its exit status. */
export function artifactize(fixture: Fixture, ...args: string[]): number {
  const result = spawnSync(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, ...args], {
    env: environment(fixture),
    encoding: "utf8",
  });
  if (result.error) throw result.error;
  return result.status ?? -1;
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
export function remoteStatus(fixture: Fixture): Promise<{ code: number; stdout: string }> {
  return new Promise((done, fail) => {
    execFile(cli(), ["--state-dir", fixture.state, "--repo", fixture.repo, "remote", "status", "--json"],
      { encoding: "utf8", env: { ...environment(fixture), ARTIFACTIZE_REMOTE: "" } },
      (error, stdout) => {
        if (error && typeof error.code !== "number") fail(error);
        else done({ code: typeof error?.code === "number" ? error.code : 0, stdout });
      });
  });
}

/** The saved requests, as `request list --json` prints them. */
export function requests(fixture: Fixture): Saved[] {
  const result = spawnSync(
    cli(),
    ["--repo", fixture.repo, "--state-dir", fixture.state, "request", "list", "--json"],
    { encoding: "utf8", env: environment(fixture) },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr);
  return JSON.parse(result.stdout) as Saved[];
}

/** All saved Runs through the public CLI, including non-default statuses and old entries.
 * An empty state database is allowed: absence of Runs is a semantic query, not a file test. */
export function savedRuns(fixture: Fixture): { id: string }[] {
  const result = spawnSync(
    cli(),
    ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "list", "--all", "--limit", "4294967295", "--json"],
    { encoding: "utf8", env: environment(fixture) },
  );
  if (result.error) throw result.error;
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
function declare(repo: string, notes = "# Release notes\n- Ready to ship.\n", signoff: SignoffDefinition = DEFAULT_SIGNOFF) {
  for (const [name, text] of [
    ["release", release(dirname(repo), signoff)],
    ["ship", SHIP],
    ["docs", DOCS],
  ]) {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(join(repo, name, "index.artf"), text);
  }
  writeFileSync(join(repo, "release", "notes.md"), notes);
  git(repo, "init", "-q", "--initial-branch=master", repo);
}

/** Submit every eval and observe its saved scheduler Human boundary. */
export function verifyWaiting(fixture: Fixture): Promise<string> {
  return waitingRun(cli(), fixture);
}

/** Current declarations only: no verify and no saved Run. */
export function createGraph(): Fixture {
  return prepareFixture((root) => {
    const repo = join(root, "repo");
    declare(repo);
    return { root, repo, state: join(root, "state") };
  });
}

/** A fresh repository and state with one Run waiting for the Human sign-off. */
export async function create(signoff: SignoffDefinition = DEFAULT_SIGNOFF): Promise<Fixture> {
  return prepareFixture(async (root) => {
    const repo = join(root, "repo");
    const state = join(root, "state");
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
export async function addRepository(fixture: Fixture, name: string, signoff: SignoffDefinition = DEFAULT_SIGNOFF): Promise<string> {
  const repo = join(fixture.root, name);
  declare(repo, `# Release notes of ${name}\n- Ready to ship.\n`, signoff);
  await verifyWaiting({ ...fixture, repo });
  return repo;
}

function humanExample(repo: string, name: string, criterion: string, title: string, tags: string[] = []): void {
  const directory = join(repo, name);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "index.artf"), `name = ${JSON.stringify(name)}
tags = ${JSON.stringify(tags)}

[evals.${criterion}]
title = ${JSON.stringify(title)}
profile = { kind = "human" }

[evals.${criterion}.payload]
instruction = ${JSON.stringify(`Review ${title}.`)}
`);
}

/** Three otherwise comparable Human records: ignoring any tested filter leaves
 * extra rows. Expected identities come from the submitted Run, never from search. */
export async function addSearchExamples(fixture: Fixture): Promise<{
  baseQuery: string; allIds: string[];
  tag: { term: string; id: string }; status: { term: string; id: string }; text: { term: string; id: string };
}> {
  const repo = join(fixture.root, "search-examples");
  humanExample(repo, "amber", "check", "AmberOnly example", ["search:target"]);
  humanExample(repo, "blue", "check", "Blue example", ["search:other"]);
  humanExample(repo, "cyan", "check", "Cyan example", ["search:other"]);
  git(repo, "init", "-q", "--initial-branch=master", repo);
  const target = { ...fixture, repo };
  const run = await verifyWaiting(target);
  const saved = requests(target).filter((request) => request.runId === run);
  const named = (evalId: string) => {
    const request = saved.find((request) => request.evalId === evalId);
    if (!request) throw new Error(`missing search example ${evalId}`);
    return request.id;
  };
  const red = named("blue/check");
  for (const args of [["request", "claim", red, "--reviewer", "alice"],
    ["request", "submit", red, "--reviewer", "alice", "--verdict", "RED", "--fields", "{}"]]) {
    if (artifactize(target, ...args) !== 0) throw new Error(`cannot prepare RED search example: ${args.join(" ")}`);
  }
  return {
    baseQuery: "kind:human repo:search-examples", allIds: saved.map((request) => request.id),
    tag: { term: "tag:search:target", id: named("amber/check") },
    status: { term: "status:red", id: red }, text: { term: "amber/check", id: named("amber/check") },
  };
}

function deferredHuman(fixture: Fixture): string {
  const submitted = spawnSync(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state,
    "verify", "--all", "--defer-human", "--wait", "--json"], { encoding: "utf8", env: environment(fixture) });
  if (submitted.error) throw submitted.error;
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
    const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
    humanExample(fixture.repo, "subject", "first-criterion", "First navigation criterion");
    git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
    const first = deferredHuman(fixture);
    humanExample(fixture.repo, "subject", "second-criterion", "Second navigation criterion");
    const second = deferredHuman(fixture);
    const other = { ...fixture, repo: join(root, "other") };
    humanExample(other.repo, "elsewhere", "other-criterion", "Other scope criterion");
    git(other.repo, "init", "-q", "--initial-branch=master", other.repo);
    const third = deferredHuman(other);
    // All writers are joined. These explicit fixture times determine newest-first
    // navigation independently of wall-clock resolution and scheduling.
    const saved = requests(fixture);
    const db = new DatabaseSync(join(fixture.state, "state.sqlite"));
    try {
      db.exec("PRAGMA busy_timeout=5000");
      for (const [id, created] of [[first, "2000-01-01T00:00:00.000000000Z"],
        [second, "2000-01-02T00:00:00.000000000Z"], [third, "2000-01-03T00:00:00.000000000Z"]]) {
        db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, id);
        for (const request of saved.filter((request) => request.runId === id)) {
          db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, request.id);
        }
      }
    } finally { db.close(); }
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
    const repo = join(fixture.root, name);
    git(fixture.repo, "-C", fixture.repo, "worktree", "add", "-q", "-b", name, repo);
    writeFileSync(join(repo, "release", "notes.md"), `# Notes of ${name}\n- Distinct consumer inputs.\n`);
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
    const repo = join(root, "repo");
    const state = join(root, "state");
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
      mkdirSync(join(repo, name(index)), { recursive: true });
      writeFileSync(join(repo, name(index), "index.artf"), text);
    }
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
    if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
    return fixture;
  });
}

/** Two given chronological Run batches, with independently ordered request identities. */
export function createPagedRequests(): { fixture: Fixture; expected: string[] } {
  const fixture = createChain(100);
  return prepareFixture(() => {
    const older = requests(fixture);
    const completed = spawnSync(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state,
      "verify", "--wait", "--artifacts", "n000,n001,n002", "--json"], {
      encoding: "utf8", env: environment(fixture),
    });
    if (completed.error) throw completed.error;
    if (completed.status !== 0) throw new Error(completed.stderr || completed.stdout);
    const newer = (JSON.parse(completed.stdout) as { requests: Saved[] }).requests;
    // --wait observes terminal publication, not worker exit. Join every writer before
    // installing the controlled chronology, so finalization cannot overwrite its inputs.
    settleRuns(cli(), fixture.repo, fixture.state);
    // This is fixture setup, not an observation: explicit times make the two batches
    // genuinely older/newer without depending on clock resolution or elapsed time.
    const db = new DatabaseSync(join(fixture.state, "state.sqlite"));
    try {
      db.exec("PRAGMA busy_timeout=5000");
      for (const [batch, created] of [[older, "2000-01-01T00:00:00.000000000Z"],
        [newer, "2000-01-02T00:00:00.000000000Z"]] as const) {
        db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, batch[0].runId);
        for (const request of batch) db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, request.id);
      }
    } finally { db.close(); }
    const identity = (batch: Saved[], count: number) => Array.from({ length: count }, (_, index) => {
      const evalId = `n${String(index).padStart(3, "0")}/check`;
      const request = batch.find((request) => request.evalId === evalId);
      if (!request) throw new Error(`missing fixture request ${evalId}`);
      return request.id;
    });
    return { fixture, expected: [...identity(newer, 3), ...identity(older, 100)] };
  }, fixture.root);
}

/**
 * A fresh repository with one Artifact in each state of a Run that is not simply GREEN:
 * `red` fails, `crash` exceeds its runtime deadline (ERROR), `down` is blocked by `red`, `after` by
 * `crash`, `fine` is GREEN, and `later` waits for the Human sign-off of `gate`. Three of them have no dependency, so they share the graph's first column.
 */
export async function createStates(): Promise<Fixture> {
  return prepareFixture(async (root) => {
    const repo = join(root, "repo");
    const state = join(root, "state");
    const artifact = (name: string, operation: Operation, instruction: string, timeoutMs?: number) => {
      mkdirSync(join(repo, name), { recursive: true });
      writeFileSync(
        join(repo, name, "index.artf"),
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
    mkdirSync(join(repo, "gate"), { recursive: true });
    writeFileSync(
      join(repo, "gate", "index.artf"),
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
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    // RED/ERROR are committed while the independent Human sign-off keeps the Run waiting.
    await verifyWaiting(fixture);
    return fixture;
  });
}

/**
 * A fresh repository whose one Artifact `app` has an eval for each of `names`, all verified
 * GREEN: a zoomed Artifact with several evals to move through.
 */
export function createEvals(names: string[]): Fixture {
  return prepareFixture((root) => {
    const repo = join(root, "repo");
    const state = join(root, "state");
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
    mkdirSync(join(repo, "app"), { recursive: true });
    writeFileSync(join(repo, "app", "index.artf"), `name = "app"\n${evals}`);
    git(repo, "init", "-q", "--initial-branch=master", repo);
    const fixture = { root, repo, state };
    const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
    if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
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
    const repo = join(root, "repo");
    mkdirSync(repo, { recursive: true });
    // The caller invokes this only after the service/app has exited.
    const remove = () => releaseFixtures([root]);
    return { root, repo, state: join(root, "state"), remove };
  });
}

if (import.meta.main) {
  const fixture = await create();
  console.log(JSON.stringify(fixture));
}
