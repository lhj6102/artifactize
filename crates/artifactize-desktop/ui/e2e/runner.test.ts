// Exercise the inherited-stdio suite boundary in an otherwise idle Node runner.
import assert from "node:assert/strict";
import { it } from "node:test";
import { runnerFixture } from "./build-fixtures.ts";
import { withCleanup } from "./owned.ts";

for (const fails of [false, true]) {
  it(`joins a real ${fails ? "failed" : "successful"} suite with no incidental parent handles`, async () => {
    const fixture = await runnerFixture(fails);
    const { owner } = fixture;
    await withCleanup(async () => {
      assert.deepEqual(await owner.closed, { code: fails ? 1 : 0, killed: false }, owner.errors);
      assert.equal(fixture.completed(), "work completed", "the child really completed before the runner's exit");
      assert.doesNotMatch(owner.errors, /unsettled top-level await/);
      const complete = fixture.completeLog();
      if (fails) {
        assert.match(owner.errors, /actual suite failure/, "the real late failure remains visible after native output");
        assert.match(complete, /NATIVE_NOISE_START/);
        assert.match(complete, /NATIVE_NOISE_END/);
        assert.match(complete, /actual suite failure/);
        assert.ok(complete.length > 160000, "native diagnostics are complete, not bounded engine capture");
        assert.doesNotMatch(owner.errors, /NATIVE_NOISE_START/, "the bounded view references rather than duplicates native logs");
      } else assert.match(owner.output, /suite joined/);
      assert.match(owner.errors, /tests 1/);
      assert.match(owner.errors, fails ? /fail 1/ : /pass 1/);
    }, () => fixture.stop());
  });
}
