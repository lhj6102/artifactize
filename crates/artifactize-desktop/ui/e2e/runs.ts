// Domain Run cleanup uses public cancellation/views and the shared Rust OS observer.
// It never scans processes, signals workers, or controls the application lifecycle.
import { readCandidate } from "./build.ts";
import { environment } from "./environment.ts";
import { childDirectories, exists, nativePath, parentOf, removeTree, runProgram, runProgramOk, type NativePath } from "./platform.ts";

export type Identity = { pid: number; start_time: string };
type RunView = {
  status: string;
  control?: { worker: { owner?: Identity; launcher?: Identity }; supervisor?: Identity };
};

/** A watchdog retains inputs on a failed join; it never infers exit from a delay. */
const EXIT_BOUND = 30000;
const RECONCILE_INTERVAL = 25;
const pause = new Int32Array(new SharedArrayBuffer(4));
const TERMINAL = ["GREEN", "RED", "ERROR", "INCOMPLETE", "SIGNOFFS_PENDING"];
const VIEW_CODES = [0, 1, 2, 4, 5];

/** Discover only fixture-owned state databases, never processes or arbitrary paths. */
export function states(root: NativePath): NativePath[] {
  return childDirectories(root).filter((state) => exists(nativePath(state, "state.sqlite")));
}

function identities(view: RunView): Identity[] {
  const control = view.control;
  return [control?.worker.owner, control?.worker.launcher, control?.supervisor].filter((owner): owner is Identity => owner !== undefined);
}

/** Saved terminal state precedes worker/subscription/SQLite shutdown. Join only the
 * exact birth identities published by those Runs through the shared Rust abstraction. */
export function exited(owners: Identity[], bound = EXIT_BOUND, observer = readCandidate().observer): void {
  if (owners.length === 0) return;
  if (!observer) throw new Error("candidate does not declare the Rust process observer; preserve fixture inputs");
  const deadline = Date.now() + bound;
  for (;;) {
    const saved: unknown = JSON.parse(runProgramOk(observer, ["alive"], { input: JSON.stringify(owners), timeout: EXIT_BOUND }));
    if (!Array.isArray(saved) || saved.length !== owners.length || saved.some((alive) => typeof alive !== "boolean")) {
      throw new Error("Rust process observer returned invalid lifetime observations");
    }
    if (saved.every((alive) => !alive)) return;
    if (Date.now() >= deadline) throw new Error("fixture Run workers did not exit; preserve their inputs");
    Atomics.wait(pause, 0, 0, RECONCILE_INTERVAL);
  }
}

export function settleRuns(program: NativePath, repo: NativePath, state: NativePath, observer = readCandidate().observer): void {
  if (!exists(nativePath(state, "state.sqlite"))) return;
  const invoke = (...args: string[]) => runProgram(program, ["--repo", repo, "--state-dir", state, ...args], {
    timeout: EXIT_BOUND, env: environment({ root: parentOf(repo), repo, state }),
  });
  const listed = invoke("run", "list", "--all", "--limit", "4294967295", "--json");
  if (listed.status !== 0) throw new Error(listed.stderr || "cannot list fixture Runs for cleanup");
  const runs = JSON.parse(listed.stdout) as { id: string }[];
  const owners: Identity[] = [];
  const failures: unknown[] = [];
  const show = (id: string, wait = false): RunView => {
    const shown = invoke("run", "show", id, ...(wait ? ["--wait"] : []), "--json");
    if (!VIEW_CODES.includes(shown.status)) throw new Error(shown.stderr || `cannot observe fixture Run ${id}`);
    // Windows birth times are u64 FILETIMEs, beyond JavaScript's safe integer
    // range. Preserve decimal digits before parsing; only Rust converts them to u64.
    const exact = shown.stdout.replace(/"start_time"\s*:\s*(\d+)/g, '"start_time":"$1"');
    const view = JSON.parse(exact) as RunView;
    owners.push(...identities(view));
    return view;
  };
  for (const { id } of runs) {
    try {
      if (show(id).status === "RUNNING") {
        const cancelled = invoke("run", "cancel", id);
        if (cancelled.status !== 0) throw new Error(cancelled.stderr || `cannot cancel fixture Run ${id}`);
      }
      const saved = show(id, true);
      if (!TERMINAL.includes(saved.status)) throw new Error(`fixture Run ${id} did not settle: ${saved.status}`);
    } catch (error) { failures.push(error); }
  }
  try { exited(owners, EXIT_BOUND, observer); } catch (error) { failures.push(error); }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "fixture Run settlement and worker join failed");
}

/** Settle domain workers without deleting inputs: failed native admission/shutdown
 * still must not leave the fixture's already-submitted Runs alive. */
export function settleFixtureRuns(program: NativePath, root: NativePath, observer = readCandidate().observer): void {
  if (!exists(root)) return;
  const failures: unknown[] = [];
  for (const state of states(root)) {
    try { settleRuns(program, nativePath(root, "repo"), state, observer); } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "fixture Run cleanup failed; preserve inputs");
}

/** Call only after the runner/service has exited the app. Domain settlement and file
 * deletion are separate, so an unconfirmed native owner can retain its inputs safely. */
export function removeFixture(program: NativePath, root: NativePath, observer = readCandidate().observer): void {
  if (!exists(root)) return;
  settleFixtureRuns(program, root, observer);
  removeTree(root);
}
