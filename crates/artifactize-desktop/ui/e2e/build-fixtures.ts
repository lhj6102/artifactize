// Domain fixture roots are inputs, not candidate leases or native lifecycle owners.
// The outer runner calls releaseFixtures only after the service/app has exited.
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { readCandidate, type Candidate } from "./build.ts";
import { removeFixture } from "./runs.ts";
import { slowTool } from "./process-fixtures.ts";
import { observe } from "./domain-fixtures/observe.ts";

function fixturesRoot(): string { return join(readCandidate().root, "fixtures"); }

export function newRoot(): string {
  const scratch = fixturesRoot();
  mkdirSync(scratch, { recursive: true });
  return mkdtempSync(join(scratch, "fixture-"));
}

/** Failed acquisition has no app owner yet, so it can settle its own domain work.
 * Preserve both the setup error and a failed cleanup, retaining inputs on failure. */
export function prepareFixture<T>(setup: (root: string) => T, root = newRoot()): T {
  const failed = (error: unknown): never => {
    try { removeFixture(readCandidate().cli, root); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "fixture setup and cleanup failed; preserve inputs"); }
    throw error;
  };
  try {
    const value = setup(root);
    return value instanceof Promise ? value.catch(failed) as T : value;
  } catch (error) { return failed(error); }
}

/** Filesystem discovery crosses WDIO worker boundaries without global exit hooks. */
export function createdRoots(): string[] {
  const scratch = fixturesRoot();
  if (!existsSync(scratch)) return [];
  return readdirSync(scratch, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith("fixture-"))
    .map((entry) => join(scratch, entry.name)).sort();
}

export async function releaseFixtures(roots: readonly string[] = createdRoots(), candidate: Candidate = readCandidate()): Promise<void> {
  const failures: unknown[] = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    try {
      if (dirname(root) !== join(candidate.root, "fixtures") || !basename(root).startsWith("fixture-") || lstatSync(root).isSymbolicLink()) {
        throw new Error(`refuse cleanup outside candidate-owned fixture roots: ${root}`);
      }
      const active = slowTool({ root, repo: join(root, "repo"), state: join(root, "state") });
      if (active) await observe(async () => !await active(), () => `tool still owns fixture ${root}; preserve inputs`);
      removeFixture(candidate.cli, root, candidate.observer);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "domain fixture cleanup failed; preserve failed inputs");
}
