// Linux WebKitGTK interruption fixture. Requires the driver's private Xvfb display,
// procfs birth identities and POSIX process groups; other native automation is not
// implemented by this harness. Acquisition, signals, output and joins stay here.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { cli } from "./fixture.ts";
import { displayListening, displayLocked, LAUNCH_TIMEOUT } from "./driver.ts";
import { groupAlive, groupIdentity, type Group, withCleanup } from "./owned.ts";
import { finishFixture } from "./runs.ts";

/** Only the declared native-only scope is excluded; missing Linux capabilities fail setup. */
export const holderUnsupported = process.platform !== "linux" && "this holder tests Linux WebKitGTK, procfs, POSIX groups and private Xvfb";

type Held = { display: string; groups: number[]; root: string };
export type HeldSession = {
  privateDisplay: boolean;
  nativeActive: () => boolean;
  displayActive: () => boolean;
  interrupt: (kind: "terminate" | "kill") => void;
  waitReleased: () => Promise<void>;
  nativeExited: () => boolean;
  displayClosed: () => boolean;
  displayUnlocked: () => boolean;
  stop: () => Promise<void>;
};

async function eventually(check: () => boolean, what: string) {
  const deadline = Date.now() + 10000;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error(`timed out: ${what}`);
    await new Promise((done) => setTimeout(done, 100));
  }
}

/** Own the holder from launch through its real close, all native users and durable Runs. */
export async function holdSession(): Promise<HeldSession> {
  if (holderUnsupported) throw new Error(holderUnsupported);
  const program = cli();
  const owner = spawn(process.execPath, [fileURLToPath(new URL("./hold-session.ts", import.meta.url))], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const lines = createInterface({ input: owner.stdout! });
  owner.stdout!.on("data", (chunk: Buffer) => process.stderr.write(chunk));
  const closed = new Promise<void>((done) => owner.once("close", () => done()));
  let root: string | undefined;
  let held: Held | undefined;
  const groups: Group[] = [];
  let interrupted = false;
  let ending: Promise<void> | undefined;
  const nativeExited = () => groups.every(({ group, start_time }) => !groupAlive(group, start_time));
  const displayClosed = () => !held || !displayListening(held.display);
  const displayUnlocked = () => !held || !displayLocked(held.display);
  const waitReleased = async () => {
    const results = await Promise.allSettled([
      ...groups.map(({ group, start_time }) => eventually(() => !groupAlive(group, start_time), `owned native group ${group} gone`)),
      eventually(displayClosed, "the private display closed"),
      eventually(displayUnlocked, "the private display unlocked"),
    ]);
    const failures = results.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
    if (failures.length) throw new AggregateError(failures, "session holder native resources did not stop");
  };
  const stop = () => ending ??= finishFixture(program, root, async () => {
    // Close admission and join the actual holder before collecting its durable Runs.
    await withCleanup(() => {
      if (owner.exitCode === null && owner.signalCode === null) owner.kill("SIGKILL");
    }, () => closed, () => { lines.close(); owner.stdout?.destroy(); }, waitReleased);
  });
  try {
    held = await new Promise<Held>((done, fail) => {
      const deadline = setTimeout(() => finish(new Error("the session holder did not become ready")), LAUNCH_TIMEOUT);
      const error = (failure: Error) => finish(failure);
      const exit = (code: number | null) => finish(new Error(`the session holder exited with ${code}`));
      function finish(failure?: Error, value?: Held) {
        clearTimeout(deadline);
        lines.off("line", line);
        owner.off("error", error);
        owner.off("exit", exit);
        if (failure) fail(failure); else done(value!);
      }
      function line(text: string) {
        try {
          const value = JSON.parse(text) as Partial<Held>;
          root = value.root ?? root;
          if (value.groups && value.display) finish(undefined, value as Held);
        } catch (failure) { finish(failure instanceof Error ? failure : new Error(String(failure))); }
      }
      lines.on("line", line);
      owner.once("error", error);
      owner.once("exit", exit);
    });
    lines.close();
    for (const group of held.groups) groups.push(groupIdentity(group));
    if (groups.length !== 2) throw new Error("the native holder must own its driver and its private display");
    return {
      privateDisplay: !!held.display,
      nativeActive: () => groups.every(({ group, start_time }) => groupAlive(group, start_time)),
      displayActive: () => !!held && displayListening(held.display),
      interrupt(kind) {
        if (ending || interrupted) throw new Error("the owned session holder is stopping");
        interrupted = true;
        if (!owner.kill(kind === "terminate" ? "SIGTERM" : "SIGKILL")) throw new Error("the holder interruption could not be delivered");
      },
      waitReleased, nativeExited, displayClosed, displayUnlocked, stop,
    };
  } catch (error) {
    return withCleanup(() => { throw error; }, stop);
  }
}
