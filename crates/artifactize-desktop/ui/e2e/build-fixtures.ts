// Domain fixture roots are inputs, not candidate leases or native lifecycle owners.
// The outer runner calls releaseFixtures only after the service/app has exited.
import { readCandidate, type Candidate } from "./build.ts";
import { awaitReleased, ProcessTreeNotJoined, releaseRecordedBrowser, removeTree, newDirectory } from "./platform.ts";
import { releaseClaudeLogins } from "./domain-fixtures/claude-code.ts";
import { childDirectories, exists, isLink, makeDirectory, nameOf, nativePath, parentOf, writeText, type NativePath } from "./portable.ts";
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
    // A program tree whose exit was not confirmed may still use the files: keep them.
    if (unconfirmedEnd(error)) {
      try { writeText(nativePath(root, RETAINED), ""); }
      catch (mark) { throw new AggregateError([error, mark], "fixture setup failed and its retention could not be marked; preserve inputs"); }
      throw error;
    }
    try { removeFixture(readCandidate().cli, root); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "fixture setup and cleanup failed; preserve inputs"); }
    throw error;
  };
  try {
    const value = setup(root);
    return value instanceof Promise ? value.catch(failed) as T : value;
  } catch (error) { return failed(error); }
}

/** Whether a failure says that some program's end was not confirmed. */
function unconfirmedEnd(error: unknown): boolean {
  if (error instanceof ProcessTreeNotJoined) return true;
  if (error instanceof AggregateError) return error.errors.some(unconfirmedEnd);
  return error instanceof Error && error.cause !== undefined && unconfirmedEnd(error.cause);
}

/** Filesystem discovery crosses WDIO worker boundaries without global exit hooks. */
export function createdRoots(): NativePath[] {
  const scratch = fixturesRoot();
  if (!exists(scratch)) return [];
  return childDirectories(scratch).filter((root) => nameOf(root).startsWith("fixture-"));
}

/** Marks a fixture kept for diagnosis, so a later cleanup of the candidate keeps it too. */
const RETAINED = "retained-for-diagnosis";

/** Stop what every fixture runs, and delete only fixtures whose app and any companion have
 * been confirmed stopped; a retained fixture keeps its files for diagnosis, and stays retained
 * for every later cleanup. Returns the fixtures kept. */
export async function releaseFixtures(roots: readonly NativePath[] = createdRoots(), candidate: Candidate = readCandidate(),
  retain: ReadonlySet<NativePath> = new Set()): Promise<NativePath[]> {
  const failures: unknown[] = [];
  const kept: NativePath[] = [];
  for (const root of roots) {
    if (!exists(root)) continue;
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
      const keep = retain.has(root) || exists(nativePath(root, RETAINED));
      if (before === failures.length && !keep) {
        await awaitReleased(root);
        removeTree(root);
      }
    } catch (error) { failures.push(error); }
    if (exists(root)) {
      kept.push(root);
      try { writeText(nativePath(root, RETAINED), ""); } catch (error) { failures.push(error); }
    }
  }
  if (failures.length === 0) return kept;
  if (failures.length === 1) throw failures[0];
  throw new AggregateError(failures, "domain fixture cleanup failed; preserve failed inputs");
}
