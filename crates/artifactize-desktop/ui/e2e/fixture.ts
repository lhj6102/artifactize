// Fixture state for the end-to-end tests, built only through the real `artifactize` CLI:
// a release Artifact with a waiting Human sign-off, its tools and owner schemas, and a
// dependent Artifact that waits for it.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const crate = resolve(import.meta.dirname, "../..");
const target = resolve(crate, "../../target");
/** The CLI and the app the tests run; `cargo build -p artifactize -p artifactize-desktop`. */
export const cli = join(target, "debug", "artifactize");
export const app = join(target, "debug", "artifactize-desktop");
/** Scratch space inside the build directory, never the system temporary root. */
const scratch = join(target, "e2e");

/** Every fixture root made by this process: all of them go when it ends, however its tests
 * ended, so a run leaves nothing in the build directory. */
const made: string[] = [];
process.on("exit", () => {
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

const RELEASE = `name = "release"
fingerprint = { files = ["."] }

[views.human_tools]
notes = { description = "Print the release notes of {artifactName}.", kind = "output", command = "cat", args = ["{artifactPath}/notes.md"] }
open = { description = "Open {artifactName}.", kind = "launch", command = "true", args = ["{artifactPath}"] }
fail = { description = "Fail on purpose.", kind = "output", command = "false", args = [] }

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

/** The saved Human sign-off request. */
export function signoff(fixture: Fixture): Saved {
  const found = requests(fixture).find((request) => request.evalId === "release/signoff");
  if (!found) throw new Error("no release/signoff request");
  return found;
}

/** Declare the release, ship and docs Artifacts in a new Git repository at `repo`. */
function declare(repo: string, notes = "# Release notes\n- Ready to ship.\n") {
  for (const [name, text] of [
    ["release", RELEASE],
    ["ship", SHIP],
    ["docs", DOCS],
  ]) {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(join(repo, name, "index.artf"), text);
  }
  writeFileSync(join(repo, "release", "notes.md"), notes);
  execFileSync("git", ["init", "-q", repo]);
}

/** Verify every eval of `fixture`'s repository; the Human request stays waiting. */
function verify(fixture: Fixture) {
  // The Run ends INCOMPLETE (3) on its short wait.
  const status = artifactize(fixture, "verify", "--all", "--timeout-ms", "1", "--json");
  if (status !== 3) throw new Error(`verify exited ${status}, expected 3 (waiting Human)`);
}

/** A fresh repository and state with one Run waiting for the Human sign-off. */
export function create(): Fixture {
  const root = newRoot();
  const repo = join(root, "repo");
  const state = join(root, "state");
  declare(repo);
  const fixture = { root, repo, state };
  verify(fixture);
  return fixture;
}

/**
 * Another repository `name` beside the fixture's, in the same state, with the same eval IDs
 * and its own waiting sign-off (its notes differ, so it joins no other); returns its path.
 */
export function addRepository(fixture: Fixture, name: string): string {
  const repo = join(fixture.root, name);
  declare(repo, `# Release notes of ${name}\n- Ready to ship.\n`);
  verify({ ...fixture, repo });
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
  const status = artifactize(fixture, "verify", "--all", "--json");
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
  const fixture = create();
  console.log(JSON.stringify(fixture));
}
