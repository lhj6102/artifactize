// A session-private candidate must outlive all its children, but not an interrupted owner.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { startAgent } from "./agent.ts";
import { withCleanup } from "./owned.ts";

describe("an interrupted candidate owner", () => {
  for (const interruption of ["terminate", "kill"] as const) {
    it(`cleans its executable snapshot only after children stop on ${interruption}`, async () => {
      const agent = await startAgent();
      await withCleanup(async () => {
        agent.call("launch", { screen: "graph" });
        assert.match(agent.call("cli", { args: ["verify", "--all", "--json"] }), /"run": "run-/);
        const work = await agent.observeWork();
        assert.ok(work.inputsPresent, "the session's executable inputs exist while its children run");
        assert.deepEqual(work.scopes, ["graph-state", "state"], "interruption owns both exposed states");
        assert.ok(work.workersActive, "actual durable work exists before interruption");
        assert.ok(work.nativeActive, "actual native sessions exist before interruption");
        agent.interrupt(interruption);
        await work.waitReleased();
        assert.equal(work.nativeExited(), true, "the original native lifetimes stopped");
        assert.equal(work.workersExited(), true, "all observed durable work exited");
        assert.equal(work.inputsRemoved(), true, "only then were its executable inputs removed");
      }, () => agent.stop());
    });
  }
});
