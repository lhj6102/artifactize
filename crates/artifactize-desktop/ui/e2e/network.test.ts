// Exercise the Linux OS seam through its portable scenario/observation API.
import assert from "node:assert/strict";
import { it } from "node:test";
import { withCleanup } from "./owned.ts";
import { callbackNetworks, privateNetworkFailure } from "./network.ts";

it("owns two distinct callback networks on port 1455 at once, without changing the host", async () => {
  const fixture = await callbackNetworks();
  await withCleanup(async () => {
    const ready = await fixture.ready();
    assert.notEqual(ready[0].network, fixture.host.network);
    assert.notEqual(ready[1].network, fixture.host.network);
    assert.notEqual(ready[0].network, ready[1].network);
    assert.notEqual(ready[0].user, ready[1].user);
    for (const child of ready) {
      assert.notEqual(child.user, fixture.host.user);
      assert.equal(child.uid, 0);
      assert.equal(child.gid, 0);
      assert.deepEqual(child.uidMap, [0, fixture.host.uid, 1]);
      assert.deepEqual(child.gidMap, [0, fixture.host.gid, 1]);
    }
    // Both really bound the fixed callback port before either request is sent.
    fixture.callback();
    assert.deepEqual(await Promise.all(fixture.owners.map(async (owner) => (await owner.closed).code)), [0, 0]);
    for (const [index, id] of ["first", "second"].entries()) {
      assert.match(fixture.owners[index].output, new RegExp(`${id}:/auth/callback`));
    }
    assert.deepEqual(fixture.hostNow(), fixture.host, "network, user namespace and identity maps remain unchanged");
  }, () => fixture.stop());
});

it("preserves the real case diagnostics and propagates a private test's failure", async () => {
  const fixture = await privateNetworkFailure();
  await withCleanup(async () => {
    assert.equal((await fixture.owner.closed).code, 1);
    assert.match(fixture.owner.output, /the original private case/);
    assert.match(fixture.owner.output, /the original private failure/);
    assert.match(fixture.owner.output, /account journeys failed: 1/);
  }, () => fixture.stop());
});
