// Children the e2e driver starts (Xvfb, tauri-driver and, below it, WebKitWebDriver and the
// app) must never outlive the test process, however it ends: a runner abort, Ctrl+C, SIGTERM
// or SIGKILL. Each runs in its own process group with everything it starts, and a watchdog
// holds a pipe from this process: when this process ends in any way the kernel closes the
// pipe, and the watchdog ends the group (SIGTERM, so Xvfb removes its lock and socket, then
// SIGKILL). A normal stop ends the group itself and waits until it is gone.
import { type ChildProcess, type SpawnOptions, spawn } from "node:child_process";

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** The group has no process left. */
function gone(group: number): boolean {
  try {
    process.kill(-group, 0);
    return false;
  } catch {
    return true;
  }
}

function signal(group: number, name: NodeJS.Signals) {
  try {
    process.kill(-group, name);
  } catch {
    // Already gone.
  }
}

/** Seconds a group gets to end on SIGTERM before SIGKILL, here and in the watchdog. */
const GRACE = 2;

export class Owned {
  readonly child: ChildProcess;
  readonly group: number;
  readonly #watchdog: ChildProcess;

  private constructor(child: ChildProcess, watchdog: ChildProcess) {
    this.child = child;
    this.group = child.pid ?? 0;
    this.#watchdog = watchdog;
  }

  /** Start `command` in a process group of its own, ended with this process. */
  static spawn(command: string, args: string[], options: SpawnOptions): Owned {
    const child = spawn(command, args, { ...options, detached: true });
    // Its own group too, so a Ctrl+C to the terminal's group cannot end it before it acts.
    const watchdog = spawn(
      "sh",
      ["-c", `read _; kill -s TERM -- "-$0" 2>/dev/null; sleep ${GRACE}; kill -s KILL -- "-$0" 2>/dev/null`, String(child.pid ?? 0)],
      { stdio: ["pipe", "ignore", "ignore"], detached: true },
    );
    // Neither keeps this process alive; the pipe stays open until it ends.
    watchdog.unref();
    (watchdog.stdin as unknown as { unref?: () => void } | null)?.unref?.();
    child.unref();
    return new Owned(child, watchdog);
  }

  /** End the group and wait until no process of it is left. */
  async stop(): Promise<void> {
    // The watchdog's work is done here; it must never signal a group number reused later.
    this.#watchdog.kill("SIGKILL");
    if (!this.group) return;
    signal(this.group, "SIGTERM");
    for (let waited = 0; waited < GRACE * 1000 && !gone(this.group); waited += 50) await pause(50);
    if (!gone(this.group)) signal(this.group, "SIGKILL");
    for (let waited = 0; waited < 5000 && !gone(this.group); waited += 50) await pause(50);
  }
}
