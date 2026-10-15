// A Promise alone is not a Node event-loop owner. Exercise the same inherited-stdio
// suite boundary as run.ts in an otherwise idle Node process, without any native build.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { it } from "node:test";
import { cleanup, resourceRoot } from "./build.ts";

for (const fails of [false, true]) {
  it(`joins a real ${fails ? "failed" : "successful"} suite with no incidental parent handles`, async () => {
    const root = resourceRoot();
    const test = join(root, "child.test.ts");
    const marker = join(root, "completed");
    writeFileSync(test, `import { it } from "node:test";
import { writeFileSync } from "node:fs";
it("actual owned work", async () => {
  await new Promise((done) => setTimeout(done, 100));
  writeFileSync(${JSON.stringify(marker)}, "work completed");
  ${fails ? 'process.stderr.write("NATIVE_NOISE_START\\n" + "n".repeat(160000) + "\\nNATIVE_NOISE_END\\n"); throw new Error("actual suite failure");' : ""}
});\n`);
    const source = `import { runSuite } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
await runSuite(${JSON.stringify(root)}, [${JSON.stringify(test)}], process.env);
console.log("suite joined");`;
    const env: NodeJS.ProcessEnv = { ...process.env, ARTIFACTIZE_OUTPUT_DIR: join(root, "results") };
    delete env.NODE_TEST_CONTEXT;
    const owner = spawn(process.execPath, ["--input-type=module", "-e", source], { env, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    let errors = "";
    owner.stdout!.on("data", (chunk) => { output += chunk; });
    owner.stderr!.on("data", (chunk) => { errors += chunk; });
    const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
      owner.once("error", fail);
      owner.once("close", (code, signal) => done({ code, signal }));
    });
    try {
      assert.deepEqual(await closed, { code: fails ? 1 : 0, signal: null }, errors);
      assert.equal(readFileSync(marker, "utf8"), "work completed", "the child really completed before the runner's exit");
      assert.doesNotMatch(errors, /unsettled top-level await/);
      const directory = /Full desktop E2E diagnostics: (.+)/.exec(errors)?.[1];
      assert.ok(directory, "the actual results identify their complete diagnostic log");
      const complete = readFileSync(join(directory, "complete.log"), "utf8");
      if (fails) {
        assert.match(errors, /actual suite failure/, "the real late failure remains visible after native output");
        assert.match(complete, /NATIVE_NOISE_START/);
        assert.match(complete, /NATIVE_NOISE_END/);
        assert.match(complete, /actual suite failure/);
        assert.ok(complete.length > 160000, "native diagnostics are complete, not bounded engine capture");
        assert.doesNotMatch(errors, /NATIVE_NOISE_START/, "the bounded view references rather than duplicates native logs");
      } else assert.match(output, /suite joined/);
      assert.match(errors, /tests 1/);
      assert.match(errors, fails ? /fail 1/ : /pass 1/);
    } finally {
      if (owner.exitCode === null && owner.signalCode === null) owner.kill("SIGKILL");
      await closed;
      await cleanup(root);
    }
  });
}
