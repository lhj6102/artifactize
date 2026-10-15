// Children the e2e driver starts (Xvfb, tauri-driver and, below it, WebKitWebDriver and the
// app) are Linux-native (POSIX groups and pipe watchdogs). They must never outlive the test
// process, however it ends: a runner abort, Ctrl+C, SIGTERM
// or SIGKILL. Each runs in its own process group with everything it starts, and a watchdog
// holds a pipe from this process: when this process ends in any way the kernel closes the
// pipe, and the watchdog ends and joins the group (SIGTERM, so Xvfb removes its lock and
// socket, then SIGKILL). Native work is admitted only after registry + watchdog ACK;
// before then an admission pipe's EOF self-terminates the gate. Linux /bin/sh and
// /bin/sleep are required OS helpers, independent of the target's PATH.
import { type ChildProcess, type SpawnOptions, spawn } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type Writable } from "node:stream";

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

/** Capture the acquired lifetime before publishing it to another cleanup owner. */
export function groupIdentity(group: number): Group {
  const start_time = groupBirth(group);
  if (start_time === null) throw new Error(`owned process group ${group} exited before registration`);
  return { group, start_time };
}

/** Register a native session's groups on its fixture, before the first observation. */
export function registerGroup(root: string, group: number): void {
  appendFileSync(join(root, "native-groups"), JSON.stringify(groupIdentity(group)) + "\n");
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
/** Parent and orphan watchdog observe the same bounded actual-exit boundary. */
const POLL = 50;
const JOIN_TIMEOUT = 5000;

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
  #watchdogDisarmed = false;
  #birth: number | null = null;
  readonly #closed: Promise<void>;
  readonly #admission: Writable | null;
  #watchdogClosed: Promise<void> = Promise.resolve();
  readonly #setupFailures: unknown[] = [];
  #ending: Promise<void> | undefined;

  private constructor(child: ChildProcess, admission: Writable | null) {
    this.child = child;
    this.group = child.pid ?? 0;
    this.#admission = admission;
    // close follows exit AND draining inherited/pipe stdio. Keep the actual child
    // referenced: awaiting a Promise alone does not keep Node's event loop alive.
    this.#closed = new Promise((done) => child.once("close", () => done()));
    // Keep native spawn errors too; setup failures emitted for launch waiters must
    // not be recorded twice when stop() reports the same acquisition failure.
    child.on("error", (error) => {
      if (!this.#setupFailures.includes(error)) this.#setupFailures.push(error);
      this.#admission?.destroy();
      void this.stop().catch(() => {});
    });
    admission?.on("error", (error) => { if (!this.#ending) this.#setupFailed(error); });
  }

  /** Gate the detached lifetime before it can acquire native resources. EOF before
   * admission exits the gate; after admission an acknowledged watchdog owns cleanup. */
  static spawn(command: string, args: string[], options: SpawnOptions): Owned {
    const stdio = Array.isArray(options.stdio) ? [...options.stdio] : Array(3).fill(options.stdio ?? "pipe");
    while (stdio.length < 3) stdio.push("pipe");
    // Preserve existing extra descriptors, notably Xvfb's -displayfd 3.
    const admission = stdio.length;
    stdio.push("pipe");
    const target = options.shell
      ? [typeof options.shell === "string" ? options.shell : "/bin/sh", "-c", [command, ...args].join(" ")]
      : [command, ...args];
    const child = spawn("/bin/sh", ["-c", `IFS= read -r admitted <&${admission} || exit 125
[ "$admitted" = start ] || exit 125
exec ${admission}<&-
exec "$@"`, "owned-admission", ...target], { ...options, shell: false, detached: true, stdio });
    const owned = new Owned(child, child.stdio[admission] as Writable | null);
    if (!child.pid) return owned;
    try {
      const { start_time: birth } = groupIdentity(child.pid);
      owned.#birth = birth;
      // Fixed Linux OS helpers are independent of the target's restricted PATH.
      // The watchdog arms before native exec and retains the same birth-aware group.
      const watchdog = spawn(
        "/bin/sh",
        ["-c", `birth=$1
owns() {
  [ -e "/proc/$0/stat" ] || return 0
  read -r stat < "/proc/$0/stat" || return 1
  tail=\${stat##*) }
  set -- $tail
  shift 19
  [ "$1" = "$birth" ]
}
# dash requires the explicit -s form when signal zero precedes -- and a group operand.
alive() { owns && kill -s 0 -- "-$0" 2>/dev/null; }
printf 'armed\\n'
read _
alive && kill -s TERM -- "-$0" 2>/dev/null
waited=0
while alive && [ "$waited" -lt ${GRACE * 1000 / POLL} ]; do /bin/sleep ${POLL / 1000}; waited=$((waited+1)); done
alive && kill -s KILL -- "-$0" 2>/dev/null
waited=0
while alive && [ "$waited" -lt ${JOIN_TIMEOUT / POLL} ]; do /bin/sleep ${POLL / 1000}; waited=$((waited+1)); done
if alive; then printf 'owned group %s did not stop\\n' "$0" >&2; exit 1; fi`, String(child.pid), String(birth)],
        { stdio: ["pipe", "pipe", "inherit"], detached: true },
      );
      owned.#watchdog = watchdog;
      owned.#watchdogClosed = new Promise((done) => watchdog.once("close", (code, signal) => {
        done();
        if (!owned.#watchdogDisarmed) owned.#setupFailed(new Error(`owned watchdog ended before release: ${code ?? signal}`));
      }));
      watchdog.once("error", (error) => owned.#setupFailed(error));
      watchdog.stdout!.once("error", (error) => owned.#setupFailed(error));
      let acknowledgement = "";
      let armed = false;
      watchdog.stdout!.on("data", (chunk: Buffer) => {
        if (armed) return;
        acknowledgement += chunk;
        if (!acknowledgement.includes("\n")) return;
        armed = true;
        if (acknowledgement !== "armed\n") { owned.#setupFailed(new Error("owned watchdog did not acknowledge cleanup")); return; }
        // Group registration below is synchronous and complete before this callback.
        // Cancellation/setup failure closes admission permanently, including late ACKs.
        if (!owned.#ending && owned.#setupFailures.length === 0) owned.#admission?.end("start\n");
      });
      // The gate/work keeps Node alive. The acknowledged fallback must not keep an
      // otherwise exiting owner alive; its input EOF is the death notification.
      watchdog.unref();
      (watchdog.stdin as unknown as { unref?: () => void } | null)?.unref?.();
      (watchdog.stdout as unknown as { unref?: () => void } | null)?.unref?.();
      const resources = options.env?.DESKTOP_TEST_RESOURCES ?? process.env.DESKTOP_TEST_RESOURCES;
      if (resources) appendFileSync(join(resources, "groups"), JSON.stringify({ group: child.pid, start_time: birth }) + "\n");
    } catch (error) { owned.#setupFailed(error); }
    return owned;
  }

  #setupFailed(error: unknown): void {
    if (!this.#setupFailures.includes(error)) this.#setupFailures.push(error);
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
    this.#admission?.destroy();
    if (this.group) {
      const alive = () => groupAlive(this.group, this.#birth ?? undefined);
      if (alive()) signal(this.group, "SIGTERM");
      for (let waited = 0; waited < GRACE * 1000 && alive(); waited += POLL) await pause(POLL);
      if (alive()) signal(this.group, "SIGKILL");
      for (let waited = 0; waited < JOIN_TIMEOUT && alive(); waited += POLL) await pause(POLL);
      if (alive()) throw new Error(`owned process group ${this.group} did not stop`);
    }
    await this.#closed;
    // Only disarm the fallback after successful shutdown. Join its close too rather
    // than treating delivery of SIGKILL as acknowledgement of cleanup.
    this.#watchdog?.ref();
    this.#watchdogDisarmed = this.#watchdog?.kill("SIGKILL") ?? false;
    await this.#watchdogClosed;
    this.#watchdog?.stdin?.destroy();
    if (this.#setupFailures.length === 1) throw this.#setupFailures[0];
    if (this.#setupFailures.length > 1) throw new AggregateError(this.#setupFailures, "owned process setup failed");
  }
}
