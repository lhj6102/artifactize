// Cargo's desktop test feature may replace the shared binary while real windows are alive.
// The suite and tool session must keep using their own normal executable candidates.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { after, before, describe, it } from "node:test";
import { cargoGate, snapshotsGone } from "./build.ts";
import { create, createGraph, verifyWaiting, type Fixture } from "./fixture.ts";
import { checkBuild, LAUNCH_TIMEOUT, start, type Session } from "./driver.ts";
import { Owned } from "./owned.ts";

describe("native Cargo and real desktop candidates", () => {
  const harness = resolve(import.meta.dirname, "../../harness/harness.mts");
  let fixture: Fixture;
  let session: Session;
  let owner: Owned | undefined;
  let env: NodeJS.ProcessEnv;
  let directory: string;
  let native: Promise<Error | undefined> | undefined;

  before(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture);
    await session.browser.$('[role="tree"]').waitForExist({ timeout: LAUNCH_TIMEOUT });
    await session.browser.waitUntil(() => session.browser.execute(() => document.querySelector('[role="tree"]')?.textContent?.includes("signoff") ?? false));
  });
  after(async () => {
    try {
      if (owner) {
        try { execFileSync(process.execPath, [harness, "stop"], { env, timeout: LAUNCH_TIMEOUT }); }
        finally { await owner.stop(); await snapshotsGone(directory); }
      }
    } finally {
      try { await session?.stop(); }
      finally { await native; }
    }
  });

  it("keeps Monitor and the Agent session usable across concurrent desktop cargo test and a relaunch", async () => {
    // No mutual exclusion: the native gate owns its frontend, while this window stays open.
    native = cargoGate("test").then(() => undefined, (error: unknown) => error instanceof Error ? error : new Error(String(error)));
    directory = join(createGraph().root, "session");
    mkdirSync(directory);
    env = { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, ARTIFACTIZE_REMOTE: "off" };
    owner = Owned.spawn(process.execPath, [harness, "start"], { env, stdio: ["ignore", "pipe", "inherit"] });
    await new Promise<void>((done, fail) => {
      const lines = createInterface({ input: owner!.child.stdout! });
      const deadline = setTimeout(() => finish(new Error("concurrent Agent session did not become ready")), LAUNCH_TIMEOUT);
      const exit = (code: number | null) => finish(new Error(`concurrent Agent session exited ${code}`));
      function finish(error?: Error) {
        clearTimeout(deadline);
        lines.close();
        owner!.child.off("exit", exit);
        if (error) fail(error); else done();
      }
      lines.once("line", (line) => line === "ready" ? finish() : finish(new Error(`unexpected readiness: ${line}`)));
      owner!.child.once("exit", exit);
    });
    const snapshot = () => execFileSync(process.execPath, [harness, "snapshot"], {
      env, input: JSON.stringify({ args: {} }), encoding: "utf8", timeout: LAUNCH_TIMEOUT,
    });
    assert.match(snapshot(), /signoff/, "the real Agent window uses its session-owned executable");
    const id = await verifyWaiting(fixture);
    await session.browser.waitUntil(() => session.browser.execute((wanted) =>
      [...document.querySelectorAll('[aria-label="Runs"] [role="option"]')].some((row) => row.textContent?.includes(wanted)), id), { timeout: LAUNCH_TIMEOUT });
    const error = await native;
    if (error) throw error;
    // The native gate has now emitted its test-feature binary. Neither an existing session
    // nor the next real launch may depend on that mutable shared executable name.
    assert.match(snapshot(), /signoff/, "the same Agent session remains usable after native tests");
    await session.stop();
    session = await start(fixture);
    await session.browser.$('[role="tree"]').waitForExist({ timeout: LAUNCH_TIMEOUT });
    await session.browser.waitUntil(() => session.browser.execute(() => document.querySelector('[role="tree"]')?.textContent?.includes("signoff") ?? false));
    assert.match(await session.browser.$('[role="tree"]').getText(), /signoff/, "a new real launch still uses the suite-owned normal candidate");
  });
});
