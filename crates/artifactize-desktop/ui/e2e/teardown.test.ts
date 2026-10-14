// A test process that ends without stopping its session, by SIGTERM or SIGKILL, still takes
// everything the session started with it: Xvfb (with its display's lock and socket),
// tauri-driver, WebKitWebDriver and the app.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { createInterface } from "node:readline";
import { describe, it } from "node:test";
import { join } from "node:path";
import { cancelRuns } from "./fixture.ts";
import { LAUNCH_TIMEOUT } from "./driver.ts";

type Held = { display: string | null; groups: number[]; root: string };

/** The holder, its stdout reader, durable Runs and root all belong to one cleanup guard. */
async function hold(): Promise<{ owner: ReturnType<typeof spawn>; held: Held; stop: () => Promise<void> }> {
  const owner = spawn(process.execPath, [new URL("./hold-session.ts", import.meta.url).pathname], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const lines = createInterface({ input: owner.stdout! });
  const exited = new Promise<void>((done) => owner.once("close", () => done()));
  let root: string | undefined;
  let held: Held | undefined;
  const stop = async () => {
    try {
      // A failed assertion must settle durable Runs while their owning state still exists,
      // before killing the holder (which cannot execute its fixture exit hook on SIGKILL).
      if (root && existsSync(root)) cancelRuns({ root, repo: join(root, "repo"), state: join(root, "state") });
    } finally {
      if (owner.exitCode === null && owner.signalCode === null) owner.kill("SIGKILL");
      await exited;
      lines.close();
      owner.stdout?.destroy();
      if (held) for (const group of held.groups) await eventually(() => !alive(group), `owned group ${group} gone`);
      // Only remove the root once the holder and its profile-writing groups are gone.
      if (root) rmSync(root, { recursive: true, force: true });
    }
  };
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
        if (failure) fail(failure);
        else done(value!);
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
    return { owner, held, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

function alive(group: number): boolean {
  try {
    process.kill(-group, 0);
    return true;
  } catch {
    return false;
  }
}

/** The display's abstract socket is still listening. */
function listening(display: string): boolean {
  return readFileSync("/proc/net/unix", "utf8").includes(`@/tmp/.X11-unix/X${display.slice(1)}`);
}

async function eventually(check: () => boolean, what: string, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("an interrupted test process", { skip: process.platform !== "linux" && "process groups and Xvfb are Linux" }, () => {
  for (const signal of ["SIGTERM", "SIGKILL"] as const) {
    it(`leaves nothing running after ${signal}`, async () => {
      const holder = await hold();
      const { owner, held } = holder;
      try {
        assert.ok(held.display, "a private display");
        assert.equal(held.groups.length, 2, "the driver's group and Xvfb's");
        for (const group of held.groups) assert.ok(alive(group), `group ${group} runs`);
        assert.ok(listening(held.display), "the display listens");
        owner.kill(signal);
        for (const group of held.groups) await eventually(() => !alive(group), `group ${group} gone after ${signal}`);
        await eventually(() => !listening(held.display!), `display ${held.display} closed`);
        await eventually(() => !existsSync(`/tmp/.X${held.display!.slice(1)}-lock`), `display ${held.display} unlocked`);
      } finally {
        await holder.stop();
      }
    });
  }
});
