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

type Held = { display: string | null; groups: number[]; root: string };

/** A test process holding a session, and what it owns. */
async function hold(): Promise<{ owner: ReturnType<typeof spawn>; held: Held }> {
  const owner = spawn(process.execPath, [new URL("./hold-session.ts", import.meta.url).pathname], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const line = await new Promise<string>((done, fail) => {
    createInterface({ input: owner.stdout! }).once("line", done);
    owner.once("exit", (code) => fail(new Error(`the session holder exited with ${code}`)));
  });
  return { owner, held: JSON.parse(line) as Held };
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
      const { owner, held } = await hold();
      assert.ok(held.display, "a private display");
      assert.equal(held.groups.length, 2, "the driver's group and Xvfb's");
      for (const group of held.groups) assert.ok(alive(group), `group ${group} runs`);
      assert.ok(listening(held.display), "the display listens");
      owner.kill(signal);
      try {
        for (const group of held.groups) await eventually(() => !alive(group), `group ${group} gone after ${signal}`);
        await eventually(() => !listening(held.display!), `display ${held.display} closed`);
        await eventually(() => !existsSync(`/tmp/.X${held.display!.slice(1)}-lock`), `display ${held.display} unlocked`);
      } finally {
        // A killed owner runs no exit hook; durable Runs need explicit cancellation.
        cancelRuns({ root: held.root, repo: join(held.root, "repo"), state: join(held.root, "state") });
        rmSync(held.root, { recursive: true, force: true });
      }
    });
  }
});
