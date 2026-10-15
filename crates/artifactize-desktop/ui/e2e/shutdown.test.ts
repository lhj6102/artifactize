// Observable shutdown seams, using declared Node/POSIX fixture processes rather than the app.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { setImmediate } from "node:timers/promises";
import { it } from "node:test";
import { cleanup, resourceRoot } from "./build.ts";
import { Owned, withCleanup } from "./owned.ts";

/** A bound on an actual fixture protocol, not a delay used to infer success. */
const PROTOCOL_TIMEOUT = 10000;
async function fileWritten(path: string): Promise<void> {
  const deadline = Date.now() + PROTOCOL_TIMEOUT;
  while (!existsSync(path)) {
    if (Date.now() >= deadline) throw new Error(`fixture did not publish ${path}`);
    await new Promise((done) => setTimeout(done, 10));
  }
}

it("rejects cancellation received while joining after the successful suite process exits", async () => {
  const directory = mkdtempSync(join(tmpdir(), "artifactize-desktop-cancel-join-"));
  const acknowledged = join(directory, "cancel-observed");
  const requested = join(directory, "shutdown-started");
  const ready = join(directory, "descendant-ready");
  const source = `import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { resourceRoot, runSuite, cleanup } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
import { withCleanup } from ${JSON.stringify(new URL("./owned.ts", import.meta.url).href)};
const root = resourceRoot();
const descendant = \`const { writeFileSync, existsSync } = require("node:fs");
process.on("SIGTERM", () => { writeFileSync(${JSON.stringify(requested)}, "joining"); process.kill(\${process.pid}, "SIGTERM"); });
writeFileSync(${JSON.stringify(ready)}, "ready");
setInterval(() => { if (existsSync(${JSON.stringify(acknowledged)})) process.exit(0); }, 5);\`;
const test = join(root, "child.test.ts");
writeFileSync(test, \`import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { it } from "node:test";
it("successful test with an owned descendant", async () => {
  const child = spawn(process.execPath, ["-e", \${JSON.stringify(descendant)}], { stdio: "ignore" });
  child.unref();
  while (!existsSync(${JSON.stringify(ready)})) await new Promise((done) => setTimeout(done, 5));
});\`);
process.once("SIGTERM", () => writeFileSync(${JSON.stringify(acknowledged)}, "cancelled"));
await withCleanup(() => runSuite(root, [test], process.env), () => cleanup(root));
console.log("suite accepted");`;
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], { env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  let errors = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  await withCleanup(async () => {
    await closed;
    assert.ok(existsSync(requested) && existsSync(acknowledged), "shutdown caused a real cancellation before the descendant released its lifetime");
    assert.match(errors, /pass 1/, "the test runner itself had already succeeded");
    assert.equal(owner.child.exitCode, 1, errors);
    assert.match(errors, /was cancelled/);
    assert.doesNotMatch(output, /suite accepted/);
  }, async () => {
    await owner.stop();
    rmSync(directory, { recursive: true, force: true });
  });
});

it("joins a nested detached owner beyond candidate removal and keeps standalone diagnostics outside the ancestor", async () => {
  const directory = mkdtempSync(join(tmpdir(), "artifactize-desktop-nested-join-"));
  const reported = join(directory, "cleanup-returned");
  const released = join(directory, "release-final-output");
  writeFileSync(join(directory, "sh"), `#!${process.execPath}
const { spawn } = require("node:child_process");
const { writeFileSync, existsSync } = require("node:fs");
const child = spawn("sh", process.argv.slice(2), { env: { ...process.env, PATH: ${JSON.stringify(process.env.PATH)} }, stdio: "inherit" });
child.once("error", (error) => { console.error(error); process.exit(1); });
child.once("close", (code) => {
  writeFileSync(${JSON.stringify(reported)}, "cleanup process exited");
  setInterval(() => {
    if (!existsSync(${JSON.stringify(released)})) return;
    writeFileSync(1, "detached final output\\n");
    process.exit(code ?? 1);
  }, 5);
});
`, { mode: 0o700 });
  const root = resourceRoot();
  const source = `import { resourceRoot } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
const root = resourceRoot();
process.stdout.write(root + "\\n", () => process.kill(process.pid, "SIGKILL"));`;
  const env: NodeJS.ProcessEnv = { ...process.env, PATH: directory, TMPDIR: join(root, "tmp"), DESKTOP_TEST_RESOURCES: root };
  delete env.ARTIFACTIZE_OUTPUT_DIR;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], { env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  let errors = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  let closing: Promise<void> | undefined;
  await withCleanup(async () => {
    await closed;
    await owner.stop();
    await fileWritten(reported);
    assert.equal(existsSync(output.trim()), false, "the nested candidate really disappeared before the detached owner exited");
    const diagnostics = /Desktop E2E cleanup diagnostics for .+: (.+)/.exec(errors)?.[1];
    assert.ok(diagnostics, errors);
    assert.ok(relative(root, diagnostics).startsWith(".."), "diagnostics are outside the removable ancestor");
    closing = cleanup(root);
    void closing.catch(() => {});
    // A controlled event-loop boundary lets an erroneously immediate cleanup complete;
    // the detached owner is explicitly held by the external fixture protocol above.
    await setImmediate();
    assert.ok(existsSync(root), "an enclosing owner cannot release scratch while detached final output is pending");
    writeFileSync(released, "release");
    await closing;
    assert.equal(existsSync(root), false);
    assert.match(readFileSync(join(diagnostics, "stdout.log"), "utf8"), /detached final output/);
    assert.equal(JSON.parse(readFileSync(join(diagnostics, "outcome.json"), "utf8")).code, 0);
  }, () => { writeFileSync(released, "release"); }, () => owner.stop(), async () => {
    await (closing ?? cleanup(root));
    rmSync(directory, { recursive: true, force: true });
  });
});
