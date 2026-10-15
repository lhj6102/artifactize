// Exercise ownership transfer without compiling or launching the native app.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";
import { it } from "node:test";
import { cleanup } from "./build.ts";
import { acknowledge } from "./handoff.ts";

for (const response of ["accept", "wrong", "EOF", "SIGTERM"] as const) {
  it(`owns offered inputs until receiving-owner ${response}`, async () => {
    const source = `import { resourceRoot, offerCandidate } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
import { writeFileSync } from "node:fs";
import { join } from "node:path";
const root = resourceRoot();
const candidate = { root, cli: join(root, "cli"), app: join(root, "app"), manifest: join(root, "candidate.json") };
writeFileSync(candidate.cli, "immutable CLI input");
writeFileSync(candidate.app, "immutable app input");
writeFileSync(candidate.manifest, JSON.stringify(candidate));
await offerCandidate(candidate);`;
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const owner = spawn(process.execPath, ["--input-type=module", "-e", source], { env, stdio: ["pipe", "pipe", "pipe"] });
    let errors = "";
    owner.stderr.on("data", (chunk) => { errors += chunk; });
    const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
      owner.once("error", fail);
      owner.once("close", (code, signal) => done({ code, signal }));
    });
    const lines = createInterface({ input: owner.stdout });
    const manifest = new Promise<string>((done, fail) => {
      lines.once("line", done);
      void closed.then(() => fail(new Error(`candidate sender exited without an offer: ${errors}`)), fail);
    });
    let root: string | undefined;
    const failures: unknown[] = [];
    try {
      const offered = await manifest;
      root = dirname(offered);
      assert.ok(existsSync(offered), "publication alone leaves the immutable input owned and present");
      assert.equal(owner.exitCode, null, "the sender cannot exit successfully before acceptance");
      if (response === "accept") {
        // The receiver persists its cleanup obligation before ACK, never inferring it
        // from successful stdout delivery or the preparer's PID.
        const responsibility = join(root, "receiving-owner.json");
        writeFileSync(responsibility, JSON.stringify({ manifest: offered, cleanup: "after actual owner exit" }));
        owner.stdin.end(`accept ${offered}\n`);
        assert.deepEqual(await closed, { code: 0, signal: null }, errors);
        assert.equal(JSON.parse(readFileSync(responsibility, "utf8")).manifest, offered);
        assert.ok(existsSync(offered), "accepted inputs survive the sender's real exit for the new owner");
      } else {
        if (response === "wrong") owner.stdin.end("accept a-different-manifest\n");
        else if (response === "EOF") owner.stdin.end();
        else owner.kill("SIGTERM");
        assert.deepEqual(await closed, { code: 1, signal: null }, errors);
        assert.ok(!existsSync(root), "failed or cancelled handoff completes cleanup before reporting failure");
        assert.match(errors, response === "wrong" ? /did not acknowledge/ : response === "EOF" ? /without accepting/ : /was cancelled/);
      }
    } catch (error) { failures.push(error); }
    try {
      owner.stdin.end();
      await closed;
      lines.close();
      if (root) await cleanup(root);
    } catch (error) { failures.push(error); }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures, "handoff assertion and cleanup failed");
  });
}

it("rejects failed manifest delivery even if the receiver has sent acceptance", async () => {
  const input = new PassThrough();
  const output = new Writable({ write(_chunk, _encoding, done) { done(new Error("receiver output failed")); } });
  const offered = acknowledge("/candidate/manifest.json", input, output, new AbortController().signal);
  input.end("accept /candidate/manifest.json\n");
  await assert.rejects(offered, /receiver output failed/);
});
