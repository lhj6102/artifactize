// Saved terminal state is not process exit; use the shared native scenario API.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { setImmediate } from "node:timers/promises";
import { nativeUnsupported } from "./native-fixtures.ts";
import { failedRunCleanupFixture, fixtureGroup, fixtureLifecycle, fixtureSnapshot, workerAdmissionFixture, workerExitFixture } from "./run-fixtures.ts";
import { withCleanup } from "./owned.ts";

/** Retain the real failing CLI/cancellation cause, not merely a cleanup wrapper. */
function failedCleanup(error: unknown): boolean {
  if (error instanceof AggregateError) return error.errors.length > 0 && error.errors.every(failedCleanup);
  return error instanceof Error && (/cannot list fixture Runs|bad option/.test(error.message) || failedCleanup(error.cause));
}

// The observed behavior exists only in the Linux procfs/POSIX native fixture layer.
// Missing required setup on supported Linux throws; it does not become a skip.
describe("Linux durable worker exit", { skip: nativeUnsupported }, () => {
  it("distinguishes an actual worker, another birth identity and an exited worker", async () => {
    const fixture = await workerExitFixture();
    await withCleanup(async () => {
      await fixture.child.line();
      assert.equal(fixture.discover(), 1);
      assert.equal(fixture.matchesWorker(), true);
      assert.equal(fixture.active(), true);
      assert.equal(fixture.anotherBirthActive(), false, "a reused PID is not the owned lifetime");
      assert.throws(() => fixture.requireExited(), /preserve their working files/, "terminal publication alone cannot make a live worker exited");
      fixture.release();
      await fixture.child.closed;
      assert.equal(fixture.active(), false);
      fixture.requireExited();
    }, () => fixture.stop());
  });

  it("waits for each requested state's live admission and captures both worker lifetimes", async () => {
    const fixture = await workerAdmissionFixture();
    await withCleanup(async () => {
      // The first state is actually ready; the second acquisition is still held by
      // the fixture protocol. Let an erroneously immediate readiness promise settle.
      await setImmediate();
      assert.equal(fixture.returned(), false, "the live Monitor worker cannot stand in for pending graph-state admission");
      await fixture.admitSecond();
      await fixture.ready();
      assert.equal(fixture.returned(), true);
      assert.deepEqual(fixture.capturedScopes(), ["monitor", "graph"], "the readiness snapshot retains both actual birth identities");
      assert.equal(fixture.capturedActive(), true, "readiness observes actual live work, not just accepted requests");
    }, () => fixture.stop());
  });

  it("removes a fixture on normal completion and assertion unwind only after shutdown", async () => {
    for (const unwind of [false, true]) {
      const fixture = await fixtureLifecycle();
      let stopped = false;
      const failure = new Error("journey assertion failed");
      await withCleanup(async () => {
        const exercise = async () => {
          try { if (unwind) throw failure; }
          finally {
            await fixture.finish(async () => {
              assert.ok(fixture.inputsPresent(), "inputs exist until the shutdown boundary");
              stopped = true;
            });
          }
        };
        if (unwind) await assert.rejects(exercise(), (error: unknown) => error === failure);
        else await exercise();
        assert.ok(stopped);
        assert.equal(fixture.rootPresent(), false);
      }, () => fixture.stop());
    }
  });

  it("preserves inputs and the original error when native shutdown fails", async () => {
    const fixture = await fixtureLifecycle();
    const failure = new Error("native shutdown failed");
    await withCleanup(async () => {
      await assert.rejects(fixture.finish(async () => { throw failure; }), (error: unknown) => error === failure);
      assert.ok(fixture.inputsPresent());
    }, () => fixture.stop());
  });

  it("preserves native inputs until the actual owned group exits, without following another birth", async () => {
    const fixture = await fixtureGroup();
    await withCleanup(async () => {
      await fixture.owner.line();
      fixture.register();
      assert.equal(fixture.anotherBirthActive(), false, "another birth identity is not this lifetime");
      assert.throws(() => fixture.remove(), /owned native group.*preserve fixture/);
      assert.ok(fixture.inputsPresent(), "a live writer retains its profile");
      await fixture.owner.stop();
      fixture.remove();
      assert.equal(fixture.rootPresent(), false);
    }, () => fixture.stop());
  });

  it("preserves a nested executable snapshot until its owner has cleaned it", async () => {
    const fixture = await fixtureSnapshot();
    await withCleanup(() => {
      assert.throws(() => fixture.remove(), /owned executable snapshots.*preserve fixture/);
      assert.ok(fixture.snapshotPresent());
    }, () => fixture.stop());
  });

  it("preserves state and executable inputs when durable-Run cleanup fails", async () => {
    const fixture = await failedRunCleanupFixture();
    await withCleanup(async () => {
      await assert.rejects(fixture.cleanup(), failedCleanup);
      let stopped = false;
      await assert.rejects(fixture.finish(async () => { stopped = true; }), failedCleanup);
      assert.equal(stopped, true, "settlement failure still shuts down the holder on unwind");
      const stopError = new Error("native stop also failed");
      await assert.rejects(fixture.finish(async () => { throw stopError; }),
        (error: unknown) => error instanceof AggregateError && error.errors.length === 2 && error.errors[0] === stopError);
      assert.ok(fixture.statePresent(), "failure retains owned state");
      assert.ok(fixture.executablePresent(), "failure retains executable inputs");
    }, () => fixture.stop());
  });
});
