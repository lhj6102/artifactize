// Exercise ownership transfer without compiling or launching the native app.
import assert from "node:assert/strict";
import { PassThrough, Writable } from "node:stream";
import { it } from "node:test";
import { handoffFixture } from "./build-fixtures.ts";
import { acknowledge } from "./handoff.ts";
import { withCleanup } from "./owned.ts";

for (const response of ["accept", "wrong", "EOF", "terminate"] as const) {
  it(`owns offered inputs until receiving-owner ${response}`, async () => {
    const fixture = await handoffFixture();
    const { owner } = fixture;
    await withCleanup(async () => {
      const offered = await fixture.offer();
      assert.ok(fixture.present(), "publication alone leaves the immutable input owned and present");
      assert.equal(owner.exitCode, null, "the sender cannot exit successfully before acceptance");
      fixture.respond(response);
      if (response === "accept") {
        assert.deepEqual(await owner.closed, { code: 0, killed: false }, owner.errors);
        assert.equal(fixture.acceptedManifest(), offered);
        assert.ok(fixture.present(), "accepted inputs survive the sender's real exit for the new owner");
      } else {
        assert.deepEqual(await owner.closed, { code: 1, killed: false }, owner.errors);
        assert.ok(!fixture.inputsPresent(), "failed or cancelled handoff completes cleanup before reporting failure");
        assert.match(owner.errors, response === "wrong" ? /did not acknowledge/ : response === "EOF" ? /without accepting/ : /was cancelled/);
      }
    }, () => fixture.stop());
  });
}

it("rejects failed manifest delivery even if the receiver has sent acceptance", async () => {
  const input = new PassThrough();
  const output = new Writable({ write(_chunk, _encoding, done) { done(new Error("receiver output failed")); } });
  const offered = acknowledge("/candidate/manifest.json", input, output, new AbortController().signal);
  input.end("accept /candidate/manifest.json\n");
  await assert.rejects(offered, /receiver output failed/);
});
