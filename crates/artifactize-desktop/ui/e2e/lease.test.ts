// Detached cleanup reports remain observable after owner death; no app build is involved.
import assert from "node:assert/strict";
import { it } from "node:test";
import { failedCleanupFixture, failedLeaseFixture } from "./build-fixtures.ts";
import { withCleanup } from "./owned.ts";

it("retains cleanup failure diagnostics and outcome after its owner is killed", async () => {
  const fixture = await failedCleanupFixture();
  await withCleanup(async () => {
    await fixture.ready();
    fixture.interrupt();
    await fixture.owner.closed;
    await fixture.owner.stop();
    const result = await fixture.report();
    assert.deepEqual(result.outcome, { code: 1, signal: null, disarmed: false });
    assert.match(result.stderr, /ENOTDIR/);
    assert.ok(fixture.inputsPresent(), "failed detached cleanup retains its inputs");
    await assert.rejects(fixture.join(), /detached candidate cleanup failed/);
    await fixture.repairAndCleanup();
    assert.ok((await fixture.report()).present, "reports survive the candidate's eventual removal");
  }, () => fixture.stop());
});

it("propagates an unexpectedly failed lease to an available handoff owner", async () => {
  const fixture = await failedLeaseFixture();
  await withCleanup(async () => {
    await fixture.finished();
    assert.equal(fixture.owner.exitCode, 1, fixture.owner.errors);
    assert.match(fixture.owner.errors, /candidate cleanup lease ended unexpectedly: 23/);
    const result = await fixture.report();
    assert.deepEqual(result.outcome, { code: 23, signal: null, disarmed: false });
    assert.match(result.stdout, /lease stdout retained/);
    assert.match(result.stderr, /lease stderr retained/);
    assert.ok(fixture.inputsPresent(), "unexpected lease failure is not treated as cleanup acknowledgement");
    await assert.rejects(fixture.join(), /detached candidate cleanup failed/);
  }, () => fixture.stop());
});
