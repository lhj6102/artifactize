// The runner's own parts: batching, fixture environments, teardown and build steps.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { it } from "node:test";
import { runStep } from "./build.ts";
import { own } from "./fixtures/owned.ts";
import { environment } from "./fixtures/repo.ts";
import { PNG } from "pngjs";
import { nodeProgram, xwdPng } from "./platform.ts";
import { exists, nativePath, removeTree, scratchDirectory, suitePath, newDirectory } from "./portable.ts";
import { batches } from "./run.ts";

it("batches scenarios in order, at most `size` apps at once, one that runs alone or is started again by itself", () => {
  const ids = (names: string) => names.split(" ").map((id) => ({ id, alone: id.startsWith("x") }));
  assert.deepEqual(batches(ids("a b c d e"), 2), [["a", "b"], ["c", "d"], ["e"]]);
  assert.deepEqual(batches(ids("a x1 b c x2"), 2), [["a"], ["x1"], ["b", "c"], ["x2"]]);
  assert.deepEqual(batches(ids("a b c"), 1), [["a"], ["b"], ["c"]]);
  const again = (names: string) => names.split(" ").map((id) => ({ id, restarted: id.startsWith("r") ? async () => {} : undefined }));
  assert.deepEqual(batches(again("a r1 b c r2 d"), 3), [["a"], ["r1"], ["b", "c"], ["r2"], ["d"]]);
});

it("keeps a display frame as PNG without a WebDriver session", () => {
  const frame = Buffer.alloc(116);
  // 100-byte fixed header, one colormap entry, one 32-bit pixel in little-endian order.
  for (const [offset, value] of [[0, 100], [4, 7], [8, 2], [16, 1], [20, 1], [44, 32], [48, 4], [56, 0xff0000], [60, 0xff00], [64, 0xff], [76, 1]]) {
    frame.writeUInt32BE(value, offset);
  }
  frame.writeUInt32LE(0x1a63b4, 112);
  const screenshot = PNG.sync.read(Buffer.from(xwdPng(frame)));
  assert.deepEqual([screenshot.width, screenshot.height, ...screenshot.data], [1, 1, 0x1a, 0x63, 0xb4, 255]);
  assert.throws(() => xwdPng(frame.subarray(0, 115)), /truncated/);
});

it("gives a fixture's programs homes inside it and none of the person's credentials", () => {
  const root = newDirectory(scratchDirectory(), "desktop-environment-test-");
  const saved = { ...process.env };
  try {
    Object.assign(process.env, { OPENAI_API_KEY: "secret", ARTIFACTIZE_REMOTE: "on", GIT_DIR: "/elsewhere", KEEP_ME: "kept" });
    const env = environment({ root });
    assert.equal(env.OPENAI_API_KEY, undefined);
    assert.equal(env.GIT_DIR, undefined);
    assert.equal(env.ARTIFACTIZE_REMOTE, "off");
    assert.equal(env.KEEP_ME, "kept");
    for (const name of ["HOME", "XDG_CONFIG_HOME", "TMPDIR"]) assert.ok(env[name]?.startsWith(root), `${name} is inside the fixture`);
    assert.equal(env.USER, "alice");
  } finally {
    for (const name of Object.keys(process.env)) if (!(name in saved)) delete process.env[name];
    Object.assign(process.env, saved);
    removeTree(root);
  }
});

it("teardown closes a gated stand-in and keeps inputs until failed cleanup is retried", async (test) => {
  const parent = newDirectory(scratchDirectory(), "desktop-owned-test-");
  test.after(() => removeTree(parent));
  const scope = own(parent);
  const order: string[] = [];
  scope.defer(() => { order.push("first"); });
  let released = false;
  scope.defer(() => { if (!released) throw new Error("a cleanup that fails"); });
  scope.defer(async () => { order.push("last"); });
  // A stand-in tool that waits for a gate nobody opens.
  const waiting = spawn(nodeProgram, [suitePath("fixtures", "program.ts"), "slow", nativePath(scope.root, "slow.gate")], { stdio: "ignore" });
  const ended = new Promise<number | null>((done, fail) => {
    waiting.once("error", fail);
    waiting.once("close", (code) => done(code));
  });
  test.after(async () => {
    if (waiting.exitCode === null && waiting.signalCode === null) waiting.kill();
    await ended;
  });
  await assert.rejects(scope.close(), (error: AggregateError) => error.errors.length === 1 && /a cleanup that fails/.test(String(error.errors[0])));
  assert.deepEqual(order, ["last", "first"]);
  assert.equal(await ended, 0, "the stand-in ended once its fixture closed");
  assert.equal(exists(scope.root), true, "a failed owner still needs the fixture");
  released = true;
  await scope.close();
  assert.deepEqual(order, ["last", "first"], "only the failed cleanup was retried");
  assert.equal(exists(scope.root), false);
});

it("returns a build step's output and fails a step that fails", async () => {
  const cwd = scratchDirectory();
  assert.equal(await runStep(nodeProgram, ["-e", "process.stdout.write('built')"], { env: process.env, cwd, capture: true }), "built");
  await assert.rejects(runStep(nodeProgram, ["-e", "process.exit(3)"], { env: process.env, cwd }), /exited 3/);
});
