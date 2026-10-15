// A test process that ends without stopping its session, by SIGTERM or SIGKILL, still takes
// everything the session started with it: Xvfb (with its display's lock and socket),
// tauri-driver, WebKitWebDriver and the app.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { describe, it } from "node:test";
import { cli } from "./fixture.ts";
import { finishFixture } from "./runs.ts";
import { groupAlive, withCleanup } from "./owned.ts";
import { displayListening, displayLocked, LAUNCH_TIMEOUT } from "./driver.ts";

type Held = { display: string | null; groups: number[]; root: string };

/** The holder, its stdout reader, durable Runs and root all belong to one cleanup guard. */
async function hold(): Promise<{ owner: ReturnType<typeof spawn>; held: Held; stop: () => Promise<void> }> {
  const owner = spawn(process.execPath, [fileURLToPath(new URL("./hold-session.ts", import.meta.url))], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const lines = createInterface({ input: owner.stdout! });
  const exited = new Promise<void>((done) => owner.once("close", () => done()));
  let root: string | undefined;
  let held: Held | undefined;
  const stop = () => finishFixture(cli(), root, async () => {
    // Always stop the holder on assertion unwind, even if durable settlement failed.
    // finishFixture removes inputs only after both settlement and this shutdown succeed.
    if (owner.exitCode === null && owner.signalCode === null) owner.kill("SIGKILL");
    await exited;
    lines.close();
    owner.stdout?.destroy();
    if (held) {
      const observed = await Promise.allSettled(held.groups.map((group) => eventually(() => !groupAlive(group), `owned group ${group} gone`)));
      const errors = observed.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
      if (errors.length) throw new AggregateError(errors, "session holder groups did not stop");
    }
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
    try { await stop(); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "session holder setup and cleanup failed"); }
    throw error;
  }
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
      await withCleanup(async () => {
        assert.ok(held.display, "a private display");
        assert.equal(held.groups.length, 2, "the driver's group and Xvfb's");
        for (const group of held.groups) assert.ok(groupAlive(group), `group ${group} runs`);
        assert.ok(displayListening(held.display), "the display listens");
        owner.kill(signal);
        for (const group of held.groups) await eventually(() => !groupAlive(group), `group ${group} gone after ${signal}`);
        await eventually(() => !displayListening(held.display!), `display ${held.display} closed`);
        await eventually(() => !displayLocked(held.display!), `display ${held.display} unlocked`);
      }, () => holder.stop());
    });
  }
});
