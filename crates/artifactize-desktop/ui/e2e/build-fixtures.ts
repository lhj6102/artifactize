// Domain fixture roots are inputs, not candidate leases or native lifecycle owners.
// The outer runner calls releaseFixtures only after the service/app has exited.
import { readCandidate, type Candidate } from "./build.ts";
import { childDirectories, exists, isLink, makeDirectory, nameOf, nativePath, newDirectory, parentOf, releaseClaudeLogins, releaseRecordedBrowser, removeTree, type NativePath } from "./platform.ts";
import { removeFixture, settleFixtureRuns } from "./runs.ts";
import { joinGatedTools, releaseToolGates, slowTool } from "./process-fixtures.ts";
import { observe } from "./domain-fixtures/observe.ts";

function fixturesRoot(): NativePath { return nativePath(readCandidate().root, "fixtures"); }

export function newRoot(): NativePath {
  const root = newDirectory(fixturesRoot(), "fixture-");
  try {
    makeDirectory(nativePath(root, "gated-tools"));
    return root;
  } catch (error) {
    try { removeTree(root); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "fixture root initialization and cleanup failed"); }
    throw error;
  }
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

/** Release only fixtures whose app and any companion have been confirmed stopped. */
export async function releaseFixtures(roots: readonly NativePath[] = createdRoots(), candidate: Candidate = readCandidate(),
  retain: ReadonlySet<NativePath> = new Set()): Promise<void> {
  const failures: unknown[] = [];
  for (const root of roots) {
    if (retain.has(root) || !exists(root)) continue;
    if (parentOf(root) !== nativePath(candidate.root, "fixtures") || !nameOf(root).startsWith("fixture-") || isLink(root)) {
      failures.push(new Error(`refuse cleanup outside candidate-owned fixture roots: ${root}`));
      continue;
    }
    const fixture = { root, repo: nativePath(root, "repo"), state: nativePath(root, "state") };
    const before = failures.length;
    try { releaseToolGates(fixture); } catch (error) { failures.push(error); }
    try { settleFixtureRuns(candidate.cli, root, candidate.observer); } catch (error) { failures.push(error); }
    try { await joinGatedTools(fixture); } catch (error) { failures.push(error); }
    try { await releaseClaudeLogins(root); } catch (error) { failures.push(error); }
    try { await releaseRecordedBrowser(root); } catch (error) { failures.push(error); }
    try {
      const active = slowTool(fixture);
      if (active) await observe(async () => !await active(), () => `tool still owns fixture ${root}; preserve inputs`);
      if (before === failures.length) removeTree(root);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "domain fixture cleanup failed; preserve failed inputs");
}
