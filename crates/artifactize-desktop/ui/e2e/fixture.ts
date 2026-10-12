// Fixture state for the end-to-end tests, built only through the real `artifactize` CLI:
// a release Artifact with a waiting Human sign-off, its tools and owner schemas, and a
// dependent Artifact that waits for it.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const crate = resolve(import.meta.dirname, "../..");
const target = resolve(crate, "../../target");
/** The CLI and the app the tests run; `cargo build -p artifactize -p artifactize-desktop`. */
export const cli = join(target, "debug", "artifactize");
export const app = join(target, "debug", "artifactize-desktop");
/** Scratch space inside the build directory, never the system temporary root. */
const scratch = join(target, "e2e");

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

/** A fresh repository and state with one Run waiting for the Human sign-off. */
export function create(): Fixture {
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, "fixture-"));
  const repo = join(root, "repo");
  const state = join(root, "state");
  for (const [name, text] of [
    ["release", RELEASE],
    ["ship", SHIP],
    ["docs", DOCS],
  ]) {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(join(repo, name, "index.artf"), text);
  }
  writeFileSync(join(repo, "release", "notes.md"), "# Release notes\n- Ready to ship.\n");
  execFileSync("git", ["init", "-q", repo]);
  const fixture = { root, repo, state };
  // The Run ends INCOMPLETE (3) on its short wait; the Human request stays waiting.
  const status = artifactize(fixture, "verify", "--all", "--timeout-ms", "1", "--json");
  if (status !== 3) throw new Error(`verify exited ${status}, expected 3 (waiting Human)`);
  return fixture;
}

if (import.meta.main) {
  const fixture = create();
  console.log(JSON.stringify(fixture));
}
