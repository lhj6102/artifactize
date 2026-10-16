// Exercise the fingerprint CLI against owned inputs outside the source checkout.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { it, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";

const program = fileURLToPath(new URL("../fingerprint.mjs", import.meta.url));

function fixture(test: TestContext) {
  const children = new Map<ReturnType<typeof execFile>, Promise<void>>();
  const root = mkdtempSync(join(process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir(), "artifactize-fingerprint-"));
  test.after(async () => {
    try {
      await Promise.all([...children].map(async ([child, closed]) => {
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
        await closed;
      }));
    } finally { rmSync(root, { recursive: true }); }
  });
  const digest = () => new Promise<string>((done, fail) => {
    const child = execFile(process.execPath, [program, root], { cwd: root, encoding: "utf8", timeout: 15000 },
      (error, stdout) => error ? fail(error) : done(stdout));
    children.set(child, new Promise<void>((done) => { child.once("close", () => done()); }));
    child.stdin!.end();
  });
  return { root, digest };
}

it("the fingerprint CLI ignores pnpm hardlink probes but still tracks genuine source changes", async (test) => {
  const { root, digest } = fixture(test);
  const source = join(root, "source.ts");
  writeFileSync(source, "export const value = 1;\n");
  const baseline = await digest();
  assert.match(baseline, /^desktop:[0-9a-f]{64}\n$/);
  // The exact basename observed in the native pnpm syscall trace, not a broad tmp prefix.
  const probe = join(root, "_tmp_3341898_19635e5c");
  writeFileSync(probe, "hardlink probe\n");
  assert.equal(await digest(), baseline);
  rmSync(probe);
  assert.equal(await digest(), baseline);
  writeFileSync(source, "export const value = 2;\n");
  assert.notEqual(await digest(), baseline);
});

it("adjacent source names and directories remain fingerprint inputs", async (test) => {
  const { root, digest } = fixture(test);
  writeFileSync(join(root, "source.ts"), "export const value = 1;\n");
  const baseline = await digest();
  for (const name of ["_tmp_42_deadbeef.ts", "_tmp_42_deadbee", "_tmp_42_DEADBEEF", "_tmp_pid_deadbeef"]) {
    const source = join(root, name);
    writeFileSync(source, "genuine source\n");
    assert.notEqual(await digest(), baseline, `${name} is not a pnpm probe`);
    rmSync(source);
  }
  // Only probe files are generated: a matching directory can still own real source.
  const directory = join(root, "_tmp_42_deadbeef");
  mkdirSync(directory);
  writeFileSync(join(directory, "source.ts"), "genuine nested source\n");
  assert.notEqual(await digest(), baseline);
});
