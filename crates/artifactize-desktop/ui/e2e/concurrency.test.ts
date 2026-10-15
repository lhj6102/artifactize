// Cargo's desktop test feature may replace the shared binary while real windows are alive.
// The suite and tool session must keep using their own normal executable candidates.
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";
import { cargoGate } from "./build.ts";
import { create, verifyWaiting, type Fixture } from "./fixture.ts";
import { startAgent, type AgentSession } from "./agent.ts";
import { checkBuild, LAUNCH_TIMEOUT, start, type Session } from "./driver.ts";
import { withCleanup } from "./owned.ts";

describe("native Cargo and real desktop candidates", () => {
  let fixture: Fixture;
  let session: Session;
  let agent: AgentSession | undefined;
  let native: Promise<Error | undefined> | undefined;

  beforeEach(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture);
    await session.browser.$('[role="tree"]').waitForExist({ timeout: LAUNCH_TIMEOUT });
    await session.browser.waitUntil(() => session.browser.execute(() => document.querySelector('[role="tree"]')?.textContent?.includes("signoff") ?? false));
  });
  afterEach(async () => {
    await withCleanup(() => agent?.stop(), () => session?.dispose(), async () => { const error = await native; if (error) throw error; });
  });

  it("keeps Monitor and the Agent session usable across concurrent desktop cargo test and a relaunch", async () => {
    // No mutual exclusion: the native gate owns its frontend, while this window stays open.
    native = cargoGate("test").then(() => undefined, (error: unknown) => error instanceof Error ? error : new Error(String(error)));
    agent = await startAgent();
    const snapshot = () => agent!.call("snapshot");
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
