// Observable shutdown seams through the shared native fixture boundary.
import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import { it } from "node:test";
import { cancelJoinFixture, nestedCleanupFixture } from "./build-fixtures.ts";
import { withCleanup } from "./owned.ts";

it("rejects cancellation received while joining after the successful suite process exits", async () => {
  const fixture = await cancelJoinFixture();
  const { owner } = fixture;
  await withCleanup(async () => {
    await owner.closed;
    assert.ok(fixture.cancellationObserved(), "shutdown caused a real cancellation before the descendant released its lifetime");
    assert.match(owner.errors, /pass 1/, "the test runner itself had already succeeded");
    assert.equal(owner.exitCode, 1, owner.errors);
    assert.match(owner.errors, /was cancelled/);
    assert.doesNotMatch(owner.output, /suite accepted/);
  }, () => fixture.stop());
});

it("joins a nested detached owner beyond candidate removal and keeps standalone diagnostics outside the ancestor", async () => {
  const fixture = await nestedCleanupFixture();
  await withCleanup(async () => {
    await fixture.owner.closed;
    await fixture.owner.stop();
    await fixture.reported();
    assert.equal(fixture.nestedInputsPresent(), false, "the nested candidate really disappeared before the detached owner exited");
    assert.ok(fixture.diagnosticsOutside(), "diagnostics are outside the removable ancestor");
    const closing = fixture.beginCleanup();
    // A controlled event-loop boundary lets an erroneously immediate cleanup complete;
    // the detached owner is explicitly held by the fixture's external release protocol.
    await setImmediate();
    assert.ok(fixture.inputsPresent(), "an enclosing owner cannot release scratch while detached final output is pending");
    fixture.release();
    await closing;
    assert.equal(fixture.inputsPresent(), false);
    const result = await fixture.report();
    assert.match(result.stdout, /detached final output/);
    assert.equal(result.outcome.code, 0);
  }, () => fixture.stop());
});
