// Fixture state for the end-to-end tests, built only through the real `artifactize` CLI:
// a release Artifact with a waiting Human sign-off, its tools and owner schemas, and a
// dependent Artifact that waits for it.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmdirSync, rmSync, watch, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const crate = resolve(import.meta.dirname, "../..");
const target = process.env.CARGO_TARGET_DIR ?? resolve(crate, "../../target");
/** The CLI and the app the tests run; `cargo build -p artifactize -p artifactize-desktop`. */
export const cli = join(target, "debug", "artifactize");
export const app = join(target, "debug", "artifactize-desktop");
/** Scratch space inside the build directory, never the system temporary root. */
const scratch = join(target, "e2e");

/** Every fixture root made by this process: all of them go when it ends, however its tests
 * ended, so a run leaves nothing in the build directory. */
const made: string[] = [];
const waiting = new Map<string, Fixture>();
process.on("exit", () => {
  // Fixture Runs now outlive their callers. Explicitly cancel before removing their state.
  for (const [run, fixture] of waiting) {
    artifactize(fixture, "run", "cancel", run);
    artifactize(fixture, "run", "show", run, "--wait");
  }
  for (const root of made) rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  // The scratch directory itself goes too, when nothing else is left in it.
  try {
    rmdirSync(scratch);
  } catch {
    // Not empty (another process's fixtures), or already gone.
  }
});

/** A new fixture root below the scratch space, removed when this process ends. */
function newRoot(): string {
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, "fixture-"));
  made.push(root);
  return root;
}

export type Fixture = { root: string; repo: string; state: string };

const release = (root: string) => `name = "release"
tags = ["type:release"]
fingerprint = { files = ["."] }

[views.human_tools]
notes = { description = "Print the release notes of {artifactName}.", kind = "output", command = "cat", args = ["{artifactPath}/notes.md"] }
open = { description = "Open {artifactName}.", kind = "launch", command = "sh", args = ["-c", "echo launched > \\"$0\\"", "${root}/launched"] }
fail = { description = "Fail on purpose.", kind = "output", command = "false", args = [] }
slow = { description = "Wait a long time.", kind = "output", command = "sleep", args = ["600"] }

[evals.signoff]
title = "A person approves the release"
profile = { kind = "human" }

[evals.signoff.payload]
instruction = """
Read the release notes and approve them."""

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
profile = { kind = "runtime", command = "true", args = [] }

[evals.check.payload]
instruction = """
Ship {release}."""
`;

const DOCS = `name = "docs"

[evals.lint]
title = "Docs build"
profile = { kind = "runtime", command = "true", args = [] }

[evals.lint.payload]
instruction = """
Build the docs."""
`;

/** Run the CLI against the fixture; returns its exit status. */
export function artifactize(fixture: Fixture, ...args: string[]): number {
  const result = spawnSync(cli, ["--repo", fixture.repo, "--state-dir", fixture.state, ...args], {
    env: { ...process.env, USER: "alice", ARTIFACTIZE_REMOTE: "off" },
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

/** The saved requests, as `request list --json` prints them. */
export function requests(fixture: Fixture): Saved[] {
  const result = spawnSync(
    cli,
    ["--repo", fixture.repo, "--state-dir", fixture.state, "request", "list", "--json"],
    { encoding: "utf8", env: { ...process.env, ARTIFACTIZE_REMOTE: "off" } },
  );
  if (result.status !== 0) throw new Error(result.stderr);
  return JSON.parse(result.stdout) as Saved[];
}

/** A killed fixture owner cannot run its exit hook; its teardown explicitly settles Runs. */
export function cancelRuns(fixture: Fixture): void {
  for (const run of new Set(requests(fixture).map((request) => request.runId))) {
    artifactize(fixture, "run", "cancel", run);
    artifactize(fixture, "run", "show", run, "--wait");
    waiting.delete(run);
  }
}

/** The saved Human sign-off request. */
export function signoff(fixture: Fixture): Saved {
  const found = requests(fixture).find((request) => request.evalId === "release/signoff");
  if (!found) throw new Error("no release/signoff request");
  return found;
}

/** Declare the release, ship and docs Artifacts in a new Git repository at `repo`. */
function declare(repo: string, notes = "# Release notes\n- Ready to ship.\n") {
  for (const [name, text] of [
    ["release", release(dirname(repo))],
    ["ship", SHIP],
    ["docs", DOCS],
  ]) {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(join(repo, name, "index.artf"), text);
  }
  writeFileSync(join(repo, "release", "notes.md"), notes);
  execFileSync("git", ["init", "-q", repo]);
}

/** Submit every eval and observe the scheduler's waiting phase, not an elapsed sleep.
 * Registration precedes the baseline read, so a change during setup cannot be missed. */
export async function verifyWaiting(fixture: Fixture): Promise<void> {
  const submitted = spawnSync(cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--json"], {
    encoding: "utf8", env: { ...process.env, ARTIFACTIZE_REMOTE: "off" },
  });
  if (submitted.status !== 0) throw new Error(submitted.stderr || "verify submission failed");
  const { run } = JSON.parse(submitted.stdout) as { run: string };
  waiting.set(run, fixture);
  await new Promise<void>((done, fail) => {
    let finished = false;
    const observed = watch(fixture.state, () => inspect());
    const deadline = setTimeout(() => finish(new Error(`Run ${run} did not reach its Human boundary`)), 30000);
    function finish(error?: Error) {
      if (finished) return;
      finished = true;
      clearTimeout(deadline);
      observed.close();
      if (error) fail(error);
      else done();
    }
    function inspect() {
      if (finished) return;
      try {
        const output = execFileSync(cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", run, "--json"], {
          encoding: "utf8", env: { ...process.env, ARTIFACTIZE_REMOTE: "off" },
        });
        const saved = JSON.parse(output) as { status: string; control: { activity: string }; requests: Saved[] };
        if (saved.status !== "RUNNING") return finish(new Error(`Run ${run} ended ${saved.status} before its Human boundary`));
        if (saved.control.activity === "waiting" && saved.requests.some((request) => request.status === "WAITING_HUMAN")) finish();
      } catch (error) {
        finish(error instanceof Error ? error : new Error(String(error)));
      }
    }
    inspect();
  });
}

/** Current declarations only: no verify and no saved Run. */
export function createGraph(): Fixture {
  const root = newRoot();
  const repo = join(root, "repo");
  declare(repo);
  return { root, repo, state: join(root, "state") };
}

/** A fresh repository and state with one Run waiting for the Human sign-off. */
export async function create(): Promise<Fixture> {
  const root = newRoot();
  const repo = join(root, "repo");
  const state = join(root, "state");
  declare(repo);
  const fixture = { root, repo, state };
  await verifyWaiting(fixture);
  return fixture;
}

/**
 * Another repository `name` beside the fixture's, in the same state, with the same eval IDs
 * and its own waiting sign-off (its notes differ, so it joins no other); returns its path.
 */
export async function addRepository(fixture: Fixture, name: string): Promise<string> {
  const repo = join(fixture.root, name);
  declare(repo, `# Release notes of ${name}\n- Ready to ship.\n`);
  await verifyWaiting({ ...fixture, repo });
  return repo;
}

/**
 * A fresh repository whose `count` Artifacts n000 … form one chain, each referring to the one
 * before it, all verified GREEN: a Run too large for the graph to draw every node at once.
 */
export function createChain(count: number): Fixture {
  const root = newRoot();
  const repo = join(root, "repo");
  const state = join(root, "state");
  const name = (index: number) => `n${String(index).padStart(3, "0")}`;
  for (let index = 0; index < count; index += 1) {
    const reference = index === 0 ? "the start" : `{${name(index - 1)}}`;
    const text = `name = "${name(index)}"

[evals.check]
title = "Check ${name(index)}"
profile = { kind = "runtime", command = "true", args = [] }

[evals.check.payload]
instruction = """
Check ${reference}."""
`;
    mkdirSync(join(repo, name(index)), { recursive: true });
    writeFileSync(join(repo, name(index), "index.artf"), text);
  }
  execFileSync("git", ["init", "-q", repo]);
  const fixture = { root, repo, state };
  const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
  if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
  return fixture;
}

/**
 * A fresh repository with one Artifact in each state of a Run that is not simply GREEN:
 * `red` fails, `crash` dies by a signal (ERROR), `down` is blocked by `red`, `after` by
 * `crash`, `fine` is GREEN, and `later` waits for the Human sign-off of `gate`. Three of them have no dependency, so they share the graph's first column.
 */
export async function createStates(): Promise<Fixture> {
  const root = newRoot();
  const repo = join(root, "repo");
  const state = join(root, "state");
  const artifact = (name: string, command: string, args: string, instruction: string) => {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(
      join(repo, name, "index.artf"),
      `name = "${name}"

[evals.check]
title = "Check ${name}"
profile = { kind = "runtime", command = "${command}", args = ${args} }

[evals.check.payload]
instruction = """
${instruction}"""
`,
    );
  };
  artifact("red", "false", "[]", "Fail.");
  artifact("crash", "sh", '["-c", "kill -TERM $$"]', "Die by a signal.");
  artifact("down", "true", "[]", "Run after {red}.");
  artifact("fine", "true", "[]", "Pass.");
  artifact("after", "true", "[]", "Run after {crash}.");
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
  artifact("later", "true", "[]", "Run after {gate}.");
  execFileSync("git", ["init", "-q", repo]);
  const fixture = { root, repo, state };
  // RED/ERROR are committed while the independent Human sign-off keeps the Run waiting.
  await verifyWaiting(fixture);
  return fixture;
}

/**
 * A fresh repository whose one Artifact `app` has an eval for each of `names`, all verified
 * GREEN: a zoomed Artifact with several evals to move through.
 */
export function createEvals(names: string[]): Fixture {
  const root = newRoot();
  const repo = join(root, "repo");
  const state = join(root, "state");
  const evals = names
    .map(
      (name) => `
[evals.${name}]
title = "Check ${name}"
profile = { kind = "runtime", command = "true", args = [] }

[evals.${name}.payload]
instruction = """
Check ${name}."""
`,
    )
    .join("");
  mkdirSync(join(repo, "app"), { recursive: true });
  writeFileSync(join(repo, "app", "index.artf"), `name = "app"\n${evals}`);
  execFileSync("git", ["init", "-q", repo]);
  const fixture = { root, repo, state };
  const status = artifactize(fixture, "verify", "--all", "--wait", "--json");
  if (status !== 0) throw new Error(`verify exited ${status}, expected 0 (GREEN)`);
  return fixture;
}

/**
 * An empty repository and state outside any Git work tree (the system temporary directory,
 * or TMPDIR): token storage refuses a state inside a repository, as the CLI does. `remove`
 * deletes it.
 */
export function createOutside(): Fixture & { remove: () => Promise<void> } {
  const root = mkdtempSync(join(tmpdir(), "artifactize-e2e-"));
  const repo = join(root, "repo");
  mkdirSync(repo, { recursive: true });
  // The closing WebView may still write its profile (below `data`) as the session ends.
  const remove = async () => {
    for (let attempt = 0; attempt < 20 && existsSync(root); attempt += 1) {
      rmSync(root, { recursive: true, force: true });
      await new Promise((done) => setTimeout(done, 100));
    }
  };
  return { root, repo, state: join(root, "state"), remove };
}

if (import.meta.main) {
  const fixture = await create();
  console.log(JSON.stringify(fixture));
}
