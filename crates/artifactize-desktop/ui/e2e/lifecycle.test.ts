// Observable fixture boundaries, through the shared Linux-native fixture API.
import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import { it } from "node:test";
import { windowClosed } from "./driver.ts";
import { withCleanup } from "./owned.ts";
import { admittedOrphanFixture, execContractFixture, gitBoundaryFixture, pendingAcquisitionFixture,
  registrationFailureFixture, staleGroupFixture, unavailableTargetFixture } from "./process-fixtures.ts";

it("requires an owned app lifetime rather than treating a session response or error as closure", async () => {
  await assert.rejects(windowClosed({ getWindowHandles: async () => [] }), /requires an owned native app session/);
  await assert.rejects(windowClosed({ getWindowHandles: async () => { throw new Error("transport failed"); } }), /requires an owned native app session/);
});

it("retains the primary error and attempts every failed owned teardown", async () => {
  const primary = new Error("the test failed");
  const native = new Error("the native stop failed");
  const lease = new Error("the lease stop failed");
  let drained = false;
  await assert.rejects(withCleanup(() => { throw primary; }, () => { throw native; }, () => {
    drained = true;
    throw lease;
  }), (error) => error instanceof AggregateError && error.errors[0] === primary && error.errors[1] === native && error.errors[2] === lease);
  assert.equal(drained, true, "an earlier teardown failure does not bypass later owners");
});

it("keeps Git repository/index/hooks/config routing inside its fixture", async () => {
  const fixture = await gitBoundaryFixture();
  await withCleanup(() => {
    fixture.exercise();
    assert.deepEqual(fixture.foreignEntries(), ["sentinel"], "the inherited repository was not initialized or modified");
    assert.equal(fixture.indexPresent(), false, "staging did not use the inherited index");
    assert.equal(fixture.hookRan(), false, "commit did not invoke inherited hooks or signing");
  }, () => fixture.stop());
});

it("does not retain a candidate for an unrelated group's reuse of a registered number", async () => {
  const fixture = await staleGroupFixture();
  await withCleanup(async () => {
    await fixture.owner.line();
    assert.ok(fixture.foreignActive(), "the independent owner is actually alive");
    await fixture.cleanupStale();
    assert.equal(fixture.inputsPresent(), false, "a different birth is not an owned user of these inputs");
    assert.ok(fixture.foreignActive(), "cleanup leaves the unrelated evaluation untouched");
  }, () => fixture.stop());
});

it("joins an acquired process even when registering its group fails", async () => {
  const fixture = await registrationFailureFixture();
  const acquisition = await fixture.acquisition;
  await withCleanup(async () => {
    assert.equal((acquisition as NodeJS.ErrnoException).code, "EISDIR");
    await assert.rejects(fixture.stopOwner(), (error) => error === acquisition);
    assert.equal(fixture.active(), false, "failed registration still joins the actual acquired group");
    assert.equal(fixture.admitted(), false, "registration failure never admits native work, including a late watchdog ACK");
  }, () => fixture.stop(acquisition));
});

it("never admits native resources when the owner dies during pending acquisition", async () => {
  const fixture = await pendingAcquisitionFixture();
  await withCleanup(async () => {
    assert.equal((await fixture.owner.closed).killed, true, fixture.owner.errors);
    await fixture.joinAcquired();
    assert.equal(fixture.admitted(), false, "owner death before armed admission cannot acquire a native display or lock");
  }, () => fixture.stop());
});

it("keeps the actual program PID, stdin, stdout, stderr and an existing readiness descriptor", async () => {
  const fixture = await execContractFixture();
  const { owner } = fixture;
  await withCleanup(async () => {
    owner.endInput("actual stdin payload");
    await owner.closed;
    assert.equal(fixture.execIdentityRetained(), true, "exec retains the acquired process identity");
    assert.equal(fixture.groupIdentityRetained(), true);
    assert.equal(owner.output, "actual stdin payload");
    assert.match(owner.errors, /actual target diagnostic/);
    assert.equal(owner.exitCode, 7, "the target's real failure status remains observable");
  }, () => fixture.stop());
});

it("reports an unavailable target immediately through its actual exit and diagnostics", async () => {
  const fixture = await unavailableTargetFixture();
  await withCleanup(async () => {
    await fixture.owner.closed;
    assert.equal(fixture.owner.exitCode, 127);
    assert.ok(fixture.namesTarget(), fixture.owner.errors);
  }, () => fixture.stop());
});

it("releases admitted native resources and inherited stderr after its owner is killed", async () => {
  const fixture = await admittedOrphanFixture();
  await withCleanup(async () => {
    assert.equal((await fixture.acquired()).resourcesPresent, true);
    assert.ok(fixture.lockPresent(), "the real target acquired a mutable resource before interruption");
    fixture.interrupt();
    await fixture.owner.exited;
    await fixture.joinNative();
    assert.equal((await fixture.owner.closed).killed, true);
    assert.equal(fixture.lockPresent(), false, "the orphan watchdog delivered termination and released the native resource");
    assert.match(fixture.owner.errors, /orphan native resource released/, "final inherited stderr was drained before actual close");
  }, () => fixture.stop());
});

it("retains enclosing scratch until the detached native user and its cleanup really join", async () => {
  const fixture = await admittedOrphanFixture(true);
  await withCleanup(async () => {
    assert.equal((await fixture.acquired()).resourcesPresent, true);
    fixture.interrupt();
    await fixture.owner.exited;
    await fixture.joinNative();
    await fixture.owner.closed;
    await fixture.cleanupReported();
    const closing = fixture.beginEnclosingCleanup();
    // Cleanup's real child has exited, but its final output/exit is explicitly held.
    // Unlike holding a termination handler, this never races a signal grace period.
    await setImmediate();
    assert.equal(fixture.nativeExited(), true, "the acquired native user really exited, not just its launcher");
    assert.equal(fixture.lockPresent(), false, "the native handler actually released its lock before cleanup was held");
    assert.ok(fixture.inputsPresent(), "enclosing cleanup preserves the detached cleanup's live release inputs");
    fixture.release();
    await closing;
    await fixture.owner.closed;
    assert.equal(fixture.nativeExited(), true);
    assert.match(fixture.owner.errors, /orphan native resource released/);
    assert.equal(fixture.inputsPresent(), false, "only joined native work and detached cleanup permit ancestor removal");
  }, () => fixture.stop());
});
