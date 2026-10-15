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

export class Owned {
  readonly child: ChildProcess;
  readonly group: number;
  readonly #watchdog: ChildProcess | null;
  readonly #birth: number | null;
  readonly #closed: Promise<void>;
  readonly #watchdogClosed: Promise<void>;
  #ending: Promise<void> | undefined;

  private constructor(child: ChildProcess, watchdog: ChildProcess | null, birth: number | null) {
    this.child = child;
    this.group = child.pid ?? 0;
    this.#watchdog = watchdog;
    this.#birth = birth;
    // close follows exit AND draining inherited/pipe stdio. Keep the actual child
    // referenced: awaiting a Promise alone does not keep Node's event loop alive.
    this.#closed = new Promise((done) => child.once("close", () => done()));
    this.#watchdogClosed = watchdog ? new Promise((done) => watchdog.once("close", () => done())) : Promise.resolve();
    // Callers observe launch errors too; ownership must still be able to join close.
    child.on("error", () => {});
    watchdog?.on("error", (error) => console.error("owned process watchdog failed", error));
  }

  /** Start `command` in a process group of its own, ended with this process. */
  static spawn(command: string, args: string[], options: SpawnOptions): Owned {
    const child = spawn(command, args, { ...options, detached: true });
    if (!child.pid) return new Owned(child, null, null);
    const birth = groupBirth(child.pid);
    const resources = options.env?.DESKTOP_TEST_RESOURCES ?? process.env.DESKTOP_TEST_RESOURCES;
    if (resources) appendFileSync(join(resources, "groups"), `${child.pid}\n`);
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
    // The watchdog must not outlive the owner by holding its event loop open, but
    // the work itself remains referenced until its real close (including stdio).
    watchdog.unref();
    (watchdog.stdin as unknown as { unref?: () => void } | null)?.unref?.();
    return new Owned(child, watchdog, birth);
  }

  /** End the group and wait until no process of it is left. Repeated stops share the
   * same lifetime, never signal a numeric group again after successful shutdown. */
  stop(): Promise<void> {
    return this.#ending ??= this.#stop();
  }

  async #stop(): Promise<void> {
    if (this.group) {
      const alive = () => groupAlive(this.group, this.#birth ?? -1);
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
  }
}
