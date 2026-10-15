// Detached cleanup reports remain observable after owner death; no app build is involved.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { it } from "node:test";
import { cleanup } from "./build.ts";
import { Owned, withCleanup } from "./owned.ts";

/** This bounds an actual detached cleanup report, not an application transition. */
const REPORT_TIMEOUT = 10000;
async function outcome(directory: string): Promise<{ code: number | null; signal: string | null; disarmed: boolean }> {
  return new Promise((done, fail) => {
    const deadline = setTimeout(() => { clearInterval(poll); fail(new Error(`cleanup did not report: ${directory}`)); }, REPORT_TIMEOUT);
    const poll = setInterval(() => {
      try {
        const result = JSON.parse(readFileSync(join(directory, "outcome.json"), "utf8"));
        clearInterval(poll);
        clearTimeout(deadline);
        done(result);
      } catch (error) {
        if (error instanceof SyntaxError || (error as NodeJS.ErrnoException).code === "ENOENT") return;
        clearInterval(poll);
        clearTimeout(deadline);
        fail(error);
      }
    }, 10);
  });
}

it("retains cleanup failure diagnostics and outcome after its owner is killed", async () => {
  const source = `import { resourceRoot } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const root = resourceRoot();
rmSync(join(root, "tmp"), { recursive: true });
writeFileSync(join(root, "tmp"), "not a directory");
process.stdout.write(root + "\\n");
process.stdin.once("data", () => process.kill(process.pid, "SIGKILL"));`;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], { env: process.env, stdio: ["pipe", "pipe", "pipe"] });
  let errors = "";
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  const lines = createInterface({ input: owner.child.stdout! });
  const offered = new Promise<string>((done, fail) => {
    lines.once("line", done);
    void closed.then(() => fail(new Error(`owner exited before readiness: ${errors}`)), fail);
  });
  let root: string | undefined;
  await withCleanup(async () => {
    root = await offered;
    owner.child.stdin!.end("interrupt");
    await closed;
    await owner.stop();
    const diagnostics = /Desktop E2E cleanup diagnostics for .+: (.+)/.exec(errors)?.[1];
    assert.ok(diagnostics, errors);
    assert.deepEqual(await outcome(diagnostics), { code: 1, signal: null, disarmed: false });
    assert.match(readFileSync(join(diagnostics, "stderr.log"), "utf8"), /ENOTDIR/);
    assert.ok(existsSync(root), "failed detached cleanup retains its inputs");
    rmSync(join(root, "tmp"));
    mkdirSync(join(root, "tmp"));
    await cleanup(root);
    assert.ok(existsSync(join(diagnostics, "outcome.json")), "reports survive the candidate's eventual removal");
  }, async () => {
    owner.child.stdin!.end("interrupt");
    await closed;
    await owner.stop();
    lines.close();
    if (root && existsSync(root)) {
      const tmp = join(root, "tmp");
      rmSync(tmp, { force: true, recursive: true });
      mkdirSync(tmp);
      await cleanup(root);
    }
  });
});

it("propagates an unexpectedly failed lease to an available handoff owner", async () => {
  const bin = mkdtempSync(join(tmpdir(), "artifactize-desktop-failed-lease-"));
  writeFileSync(join(bin, "sh"), `#!${process.execPath}\nprocess.stdout.write("lease stdout retained\\n");\nprocess.stderr.write("lease stderr retained\\n");\nprocess.exit(23);\n`, { mode: 0o700 });
  const source = `import { resourceRoot, offerCandidate } from ${JSON.stringify(new URL("./build.ts", import.meta.url).href)};
const root = resourceRoot();
console.log(JSON.stringify({ root }));
await offerCandidate({ root, cli: root + "/cli", app: root + "/app", manifest: root + "/candidate.json" });`;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], {
    env: { ...process.env, PATH: bin }, stdio: ["pipe", "pipe", "pipe"],
  });
  let output = "";
  let errors = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  let root: string | undefined;
  await withCleanup(async () => {
    await closed;
    root = (JSON.parse(output.split("\n")[0]) as { root: string }).root;
    assert.equal(owner.child.exitCode, 1, errors);
    assert.match(errors, /candidate cleanup lease ended unexpectedly: 23/);
    const diagnostics = /Desktop E2E cleanup diagnostics for .+: (.+)/.exec(errors)?.[1];
    assert.ok(diagnostics, errors);
    assert.deepEqual(await outcome(diagnostics), { code: 23, signal: null, disarmed: false });
    assert.match(readFileSync(join(diagnostics, "stdout.log"), "utf8"), /lease stdout retained/);
    assert.match(readFileSync(join(diagnostics, "stderr.log"), "utf8"), /lease stderr retained/);
    assert.ok(existsSync(root), "unexpected lease failure is not treated as cleanup acknowledgement");
  }, async () => {
    owner.child.stdin!.end();
    await owner.stop();
    if (root) await cleanup(root);
    rmSync(bin, { recursive: true, force: true });
  });
});
