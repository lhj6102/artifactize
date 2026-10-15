// Durable-Run cleanup for the Linux WebKitGTK fixture layer. Unlike `run show --wait`,
// which observes saved terminal state, this boundary also waits for the actual workers.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { groupAlive, type Group } from "./owned.ts";

export type Identity = { pid: number; start_time: number };

export function identity(pid: number): Identity {
  const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
  return { pid, start_time: Number(stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[19]) };
}

/** After the app and durable launchers are joined, even intentionally detached
 * Human tools remain owned by their fixture environment until their actual exit. */
function fixtureProcesses(root: string): Identity[] {
  return readdirSync("/proc").filter((name) => /^\d+$/.test(name)).flatMap((name) => {
    try {
      const env = readFileSync(`/proc/${name}/environ`, "utf8").split("\0");
      return env.includes(`DESKTOP_TEST_FIXTURE_ROOT=${root}`) ? [identity(Number(name))] : [];
    } catch (error) {
      if (["ENOENT", "ESRCH", "EACCES"].includes((error as NodeJS.ErrnoException).code ?? "")) return [];
      throw error;
    }
  });
}
/** Cleanup watchdog only: on expiry preserve working files and executable inputs. */
const EXIT_BOUND = 30000;
const pause = new Int32Array(new SharedArrayBuffer(4));

/** Same birth identity and non-zombie state as hey-arti's process_descriptor. The tail
 * after the final ')' begins at field 3; starttime is field 22, tail index 19. */
export function running(owner: Identity): boolean {
  try {
    const stat = readFileSync(`/proc/${owner.pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/);
    return Number(fields[19]) === owner.start_time && !["Z", "X", "x"].includes(fields[0]);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT" || (error as NodeJS.ErrnoException).code === "ESRCH") return false;
    throw error;
  }
}

/** Include the supervisor too: the saved owner is the executor after admission. The
 * exact state argument confines this read-only OS scan to this fixture's workers. */
export function workers(state: string): Identity[] {
  return readdirSync("/proc").filter((name) => /^\d+$/.test(name)).flatMap((name) => {
    try {
      const args = readFileSync(`/proc/${name}/cmdline`, "utf8").split("\0");
      const at = args.indexOf("--state-dir");
      if (at < 0 || args[at + 1] !== state || !["__verify", "__execute"].includes(args[at + 2])) return [];
      const stat = readFileSync(`/proc/${name}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/);
      return [{ pid: Number(name), start_time: Number(fields[19]) }];
    } catch (error) {
      if (["ENOENT", "ESRCH", "EACCES"].includes((error as NodeJS.ErrnoException).code ?? "")) return [];
      throw error;
    }
  });
}

/** An accepted verify may still be handing off. Observe actual worker admission before
 * an interruption scenario, instead of assuming its CLI receipt means it is running. */
export async function startedWorkers(states: string[]): Promise<Identity[]> {
  const deadline = Date.now() + EXIT_BOUND;
  for (;;) {
    const owners = states.flatMap(workers);
    if (owners.some(running)) return owners;
    if (Date.now() >= deadline) throw new Error("fixture Run workers did not start");
    await new Promise((done) => setTimeout(done, 25));
  }
}

export function exited(owners: Identity[], bound = EXIT_BOUND): void {
  const deadline = Date.now() + bound;
  while (owners.some(running)) {
    if (Date.now() >= deadline) throw new Error("owned Run workers did not exit; preserve their working files");
    Atomics.wait(pause, 0, 0, 25);
  }
}

/** A fixture may expose more than one state (Monitor and current graph). Discover each
 * actual database, including ones whose tool owner was SIGKILLed before recording a Run. */
export function states(root: string): string[] {
  return readdirSync(root, { withFileTypes: true }).filter((entry) => entry.isDirectory())
    .map((entry) => join(root, entry.name)).filter((state) => existsSync(join(state, "state.sqlite")));
}

/** Assertion unwind still stops its holder, but failed settlement or native shutdown
 * must preserve inputs. Keep both failures if shutdown also fails. */
export async function finishFixture(program: string, root: string | undefined, stop: () => Promise<void>): Promise<void> {
  const failures: unknown[] = [];
  // First close the holder's admission and join in-flight UI/CLI work. A Run list
  // collected while it can still accept verify is not a complete shutdown snapshot.
  try { await stop(); } catch (error) { failures.push(error); }
  try {
    if (root && existsSync(root)) for (const state of states(root)) settleRuns(program, join(root, "repo"), state);
  } catch (error) { failures.push(error); }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "fixture settlement and shutdown failed; preserve inputs");
  if (root) removeFixture(program, root);
}

/** The only fixture deletion boundary: failed settlement, a live native group or a
 * nested candidate retains all working files for its owner/lease to finish safely. */
export function removeFixture(program: string, root: string): void {
  if (!existsSync(root)) return;
  for (const state of states(root)) settleRuns(program, join(root, "repo"), state);
  const session = join(root, "session");
  if (existsSync(session) && readdirSync(session).some((name) => name.startsWith("candidate-"))) {
    throw new Error(`owned executable snapshots still exist; preserve fixture ${root}`);
  }
  const registry = join(root, "native-groups");
  if (existsSync(registry)) {
    for (const line of readFileSync(registry, "utf8").trim().split("\n").filter(Boolean)) {
      const owner = JSON.parse(line) as Group;
      if (!Number.isSafeInteger(owner.start_time) || owner.start_time < 0) throw new Error("invalid owned group birth identity");
      if (groupAlive(owner.group, owner.start_time)) throw new Error(`owned native group ${owner.group} still runs; preserve fixture ${root}`);
    }
  }
  exited(fixtureProcesses(root));
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

export function settleRuns(program: string, repo: string, state: string): void {
  if (!existsSync(join(state, "state.sqlite"))) return;
  const invoke = (...args: string[]) => {
    const result = spawnSync(program, ["--repo", repo, "--state-dir", state, ...args], {
      encoding: "utf8", timeout: EXIT_BOUND, env: { ...process.env, ARTIFACTIZE_REMOTE: "off" },
    });
    if (result.error) throw result.error;
    return result;
  };
  const listed = invoke("run", "list", "--all", "--limit", "4294967295", "--json");
  if (listed.status !== 0) throw new Error(listed.stderr || "cannot list fixture Runs for cleanup");
  const runs = JSON.parse(listed.stdout) as { id: string; status: string }[];
  const owners = workers(state);
  for (const { id, status } of runs) {
    if (status === "RUNNING") {
      const cancelled = invoke("run", "cancel", id);
      if (cancelled.status !== 0) throw new Error(cancelled.stderr || `cannot cancel fixture Run ${id}`);
    }
    const shown = invoke("run", "show", id, "--wait", "--json");
    // RED, ERROR and deferred Human outcomes have nonzero CLI exit codes, but still
    // publish a terminal JSON view. A command failure or timeout is not terminal.
    const saved = JSON.parse(shown.stdout) as { status: string; control?: { worker?: { owner?: Identity; launcher?: Identity } } };
    if (![0, 1, 2, 4, 5].includes(shown.status ?? -1) || !["GREEN", "RED", "ERROR", "INCOMPLETE", "SIGNOFFS_PENDING"].includes(saved.status)) {
      throw new Error(shown.stderr || `fixture Run ${id} did not settle`);
    }
    const worker = saved.control?.worker;
    if (worker?.owner) owners.push(worker.owner);
    if (worker?.launcher) owners.push(worker.launcher);
  }
  // A handoff could occur during the saved-state reads. Capture the final supervisor
  // and executor identities too, then join every one before deleting state or binaries.
  owners.push(...workers(state));
  exited(owners);
  // A recorded launcher can hand off after the first scan. It is now joined, so
  // its accepted executor cannot still be only a pending spawn when we rescan.
  exited(workers(state));
}
