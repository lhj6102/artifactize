// Domain fixture roots are inputs, not candidate leases or native lifecycle owners.
// The outer runner calls releaseFixtures only after the service/app has exited.
import { readCandidate, type Candidate } from "./build.ts";
import { childDirectories, exists, isLink, nameOf, nativePath, newDirectory, parentOf, type NativePath } from "./platform.ts";
import { removeFixture, settleFixtureRuns } from "./runs.ts";
import { slowTool } from "./process-fixtures.ts";
import { observe } from "./domain-fixtures/observe.ts";

function fixturesRoot(): NativePath { return nativePath(readCandidate().root, "fixtures"); }

export function newRoot(): NativePath {
  return newDirectory(fixturesRoot(), "fixture-");
}

/** Failed acquisition has no app owner yet, so it can settle its own domain work.
 * Preserve both the setup error and a failed cleanup, retaining inputs on failure. */
export function prepareFixture<T>(setup: (root: NativePath) => T, root = newRoot()): T {
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
export function createdRoots(): NativePath[] {
  const scratch = fixturesRoot();
  if (!exists(scratch)) return [];
  return childDirectories(scratch).filter((root) => nameOf(root).startsWith("fixture-"));
}

/** Cancel and join every fixture's Runs, keeping their files: an app not confirmed stopped may
 * still read them, but no fixture Run outlives the batch. */
export function settleFixtures(roots: readonly NativePath[] = createdRoots(), candidate: Candidate = readCandidate()): void {
  const failures: unknown[] = [];
  for (const root of roots) {
    if (!exists(root)) continue;
    try { settleFixtureRuns(candidate.cli, root, candidate.observer); } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "fixture Runs could not be settled");
}

export async function releaseFixtures(roots: readonly NativePath[] = createdRoots(), candidate: Candidate = readCandidate()): Promise<void> {
  const failures: unknown[] = [];
  for (const root of roots) {
    if (!exists(root)) continue;
    try {
      if (parentOf(root) !== nativePath(candidate.root, "fixtures") || !nameOf(root).startsWith("fixture-") || isLink(root)) {
        throw new Error(`refuse cleanup outside candidate-owned fixture roots: ${root}`);
      }
      const active = slowTool({ root, repo: nativePath(root, "repo"), state: nativePath(root, "state") });
      if (active) await observe(async () => !await active(), () => `tool still owns fixture ${root}; preserve inputs`);
      removeFixture(candidate.cli, root, candidate.observer);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "domain fixture cleanup failed; preserve failed inputs");
}
