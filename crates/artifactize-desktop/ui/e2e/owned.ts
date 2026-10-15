// Children the e2e driver starts (Xvfb, tauri-driver and, below it, WebKitWebDriver and the
// app) are Linux-native (POSIX groups and pipe watchdogs). They must never outlive the test
// process, however it ends: a runner abort, Ctrl+C, SIGTERM
// or SIGKILL. Each runs in its own process group with everything it starts, and a watchdog
// holds a pipe from this process: when this process ends in any way the kernel closes the
// pipe, and the watchdog ends the group (SIGTERM, so Xvfb removes its lock and socket, then
// SIGKILL). A normal stop ends the group itself and waits until it is gone.
import { type ChildProcess, type SpawnOptions, spawn } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { join } from "node:path";

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

export type Group = { group: number; start_time: number };

/** Candidate-wide users retain their original birth, not a later reuse of the PGID. */
export function registeredGroups(root: string): Group[] {
  return readFileSync(join(root, "groups"), "utf8").trim().split("\n").filter(Boolean).map((line) => {
    const value: unknown = JSON.parse(line);
    if (!value || typeof value !== "object") throw new Error(`invalid registered group lifetime: ${line}`);
    const { group, start_time } = value as Partial<Group>;
    if (typeof group !== "number" || !Number.isSafeInteger(group) || group <= 0 ||
        typeof start_time !== "number" || !Number.isSafeInteger(start_time) || start_time < 0) {
      throw new Error(`invalid registered group lifetime: ${line}`);
    }
    return { group, start_time };
  });
}

/** A leader birth mismatch means the numeric group belongs to a later lifetime. If the
 * original leader has exited, its remaining children still keep the group alive. */
export function groupAlive(group: number, birth?: number): boolean {
  if (!Number.isSafeInteger(group) || group <= 0) throw new Error(`invalid owned process group: ${group}`);
  if (birth !== undefined) {
    const current = groupBirth(group);
    if (current !== null && current !== birth) return false;
  }
  try {
    process.kill(-group, 0);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
    throw error;
  }
}

function groupBirth(group: number): number | null {
  try {
    const stat = readFileSync(`/proc/${group}/stat`, "utf8");
    return Number(stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[19]);
  } catch (error) {
    if (["ENOENT", "ESRCH"].includes((error as NodeJS.ErrnoException).code ?? "")) return null;
    throw error;
  }
}

/** Register a native session's groups on its fixture, before the first observation. */
export function registerGroup(root: string, group: number): void {
  const start_time = groupBirth(group);
  if (start_time === null) throw new Error(`owned process group ${group} exited before registration`);
  appendFileSync(join(root, "native-groups"), JSON.stringify({ group, start_time }) + "\n");
}

function signal(group: number, name: NodeJS.Signals) {
  try {
    process.kill(-group, name);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
  }
}

/** Seconds a group gets to end on SIGTERM before SIGKILL, here and in the watchdog. */
const GRACE = 2;

/** Keep the primary failure and attempt every owned teardown, even when an earlier
 * teardown fails. A failed release never turns a test into success. */
export async function withCleanup<T>(work: () => T | Promise<T>, ...cleanup: (() => void | Promise<void>)[]): Promise<T> {
  const failures: unknown[] = [];
  let result: T | undefined;
  try { result = await work(); } catch (error) { failures.push(error); }
  for (const release of cleanup) {
    try { await release(); } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "operation and owned cleanup failed");
  return result as T;
}

export class Owned {
  readonly child: ChildProcess;
  readonly group: number;
  #watchdog: ChildProcess | null = null;
  #birth: number | null = null;
  readonly #closed: Promise<void>;
  #watchdogClosed: Promise<void> = Promise.resolve();
  readonly #setupFailures: unknown[] = [];
  #ending: Promise<void> | undefined;

  private constructor(child: ChildProcess) {
    this.child = child;
    this.group = child.pid ?? 0;
    // close follows exit AND draining inherited/pipe stdio. Keep the actual child
    // referenced: awaiting a Promise alone does not keep Node's event loop alive.
    this.#closed = new Promise((done) => child.once("close", () => done()));
    // Callers observe launch errors too; ownership must still be able to join close.
    child.on("error", () => {});
  }

  /** Start `command` in a process group of its own, ended with this process. */
  static spawn(command: string, args: string[], options: SpawnOptions): Owned {
    const child = spawn(command, args, { ...options, detached: true });
    const owned = new Owned(child);
    if (!child.pid) return owned;
    try {
      const birth = owned.#birth = groupBirth(child.pid);
      // Its own group too, so a Ctrl+C to the terminal's group cannot end it before it acts.
      // Recheck the leader's birth before either signal: a later lifetime with the same
      // numeric PID/PGID is not ours. A missing leader may still have children in its group.
      const watchdog = spawn(
        "sh",
        ["-c", `birth=$1
  owns() {
    [ -e "/proc/$0/stat" ] || return 0
    read -r stat < "/proc/$0/stat" || return 1
    tail=\${stat##*) }
    set -- $tail
    shift 19
    [ "$1" = "$birth" ]
  }
  read _
  owns && kill -s TERM -- "-$0" 2>/dev/null
  sleep ${GRACE}
  owns && kill -s KILL -- "-$0" 2>/dev/null`, String(child.pid), String(birth ?? "")],
        { stdio: ["pipe", "ignore", "ignore"], detached: true },
      );
      owned.#watchdog = watchdog;
      owned.#watchdogClosed = new Promise((done) => watchdog.once("close", (code, signal) => {
        done();
        if (!owned.#ending) owned.#setupFailed(new Error(`owned watchdog ended before its work: ${code ?? signal}`));
      }));
      watchdog.once("error", (error) => owned.#setupFailed(error));
      // The watchdog must not outlive the owner by holding its event loop open, but
      // the work itself remains referenced until its real close (including stdio).
      watchdog.unref();
      (watchdog.stdin as unknown as { unref?: () => void } | null)?.unref?.();
      // Install ownership before registration can fail (for example a full disk).
      const resources = options.env?.DESKTOP_TEST_RESOURCES ?? process.env.DESKTOP_TEST_RESOURCES;
      if (resources) {
        if (birth === null && groupAlive(child.pid)) throw new Error(`cannot identify acquired process group ${child.pid}`);
        if (birth !== null) appendFileSync(join(resources, "groups"), JSON.stringify({ group: child.pid, start_time: birth }) + "\n");
      }
    } catch (error) { owned.#setupFailed(error); }
    return owned;
  }

  #setupFailed(error: unknown): void {
    this.#setupFailures.push(error);
    // Report to launch waiters, and retain the stop promise for their final join.
    // Observe rejection now; stop() still returns that exact failure to its owner.
    queueMicrotask(() => this.child.emit("error", error));
    void this.stop().catch(() => {});
  }

  /** End the group and wait until no process of it is left. Repeated stops share the
   * same lifetime, never signal a numeric group again after successful shutdown. */
  stop(): Promise<void> {
    return this.#ending ??= this.#stop();
  }

  async #stop(): Promise<void> {
    if (this.group) {
      const alive = () => groupAlive(this.group, this.#birth ?? undefined);
      if (alive()) signal(this.group, "SIGTERM");
      for (let waited = 0; waited < GRACE * 1000 && alive(); waited += 50) await pause(50);
      if (alive()) signal(this.group, "SIGKILL");
      for (let waited = 0; waited < 5000 && alive(); waited += 50) await pause(50);
      if (alive()) throw new Error(`owned process group ${this.group} did not stop`);
    }
    await this.#closed;
    // Only disarm the fallback after successful shutdown. Join its close too rather
    // than treating delivery of SIGKILL as acknowledgement of cleanup.
    this.#watchdog?.ref();
    this.#watchdog?.kill("SIGKILL");
    await this.#watchdogClosed;
    this.#watchdog?.stdin?.destroy();
    if (this.#setupFailures.length === 1) throw this.#setupFailures[0];
    if (this.#setupFailures.length > 1) throw new AggregateError(this.#setupFailures, "owned process setup failed");
  }
}
