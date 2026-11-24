// The runner's own parts: batching, fixture environments, teardown and build steps.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { it } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { PNG } from "pngjs";
import { BatchLifecycle } from "./batch-lifecycle.ts";
import { buildFolder, checkE2eApp, cleanup, runStep, stageCandidateBinaries, workspace } from "./build.ts";
import { own } from "./fixtures/owned.ts";
import { environment } from "./fixtures/repo.ts";
import { checkForeground, nodeProgram, prepareProcessTrees, processRunning, processState, quarantineForeground, releaseForeground, samePath, waitForProcessExit, xwdPng } from "./platform.ts";
import { exists, nativePath, newDirectory, readTextIfExists, removeTree, scratchDirectory, suitePath, withMachineLock, writeText } from "./portable.ts";
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

it("refuses candidate cleanup while a fixture has not been released", async (test) => {
  const root = newDirectory(scratchDirectory(), "desktop-candidate-test-");
  test.after(() => removeTree(root));
  const fixture = own(nativePath(root, "fixtures"));
  assert.throws(() => cleanup(root), /fixtures not released/);
  assert.equal(exists(fixture.root), true);
  await fixture.close();
  cleanup(root);
  assert.equal(exists(root), false);
});

it("returns a build step's output and fails a step that fails", async () => {
  const cwd = scratchDirectory();
  assert.equal(await runStep(nodeProgram, ["-e", "process.stdout.write('built')"], { env: process.env, cwd, capture: true }), "built");
  await assert.rejects(runStep(nodeProgram, ["-e", "process.exit(3)"], { env: process.env, cwd }), /exited 3/);
});

it("rejects an executable without the E2E build metadata before starting a session", () => {
  assert.throws(() => checkE2eApp(nodeProgram, scratchDirectory()), /not an E2E test app/);
});

it("does not admit another foreground owner after the batch process's lock unwinds without release", async (test) => {
  const root = newDirectory(scratchDirectory(), "desktop-foreground-test-");
  test.after(() => removeTree(root));
  const marker = nativePath(root, "foreground-unreleased");
  const lock = `test-foreground-${process.pid}`;
  await assert.rejects(withMachineLock(lock, async () => {
    checkForeground(marker);
    quarantineForeground(marker, "an app did not release the foreground");
    throw new Error("shutdown failed");
  }), /shutdown failed/);
  await assert.rejects(withMachineLock(lock, async () => checkForeground(marker)), /foreground is quarantined/);
  releaseForeground(marker);
  await withMachineLock(lock, async () => checkForeground(marker));
});

it("only confirms native exit after its owned child actually exits", async (test) => {
  const child = spawn(nodeProgram, ["-e", "setTimeout(() => {}, 30000)"], { stdio: "ignore" });
  const closed = new Promise<void>((done, fail) => {
    child.once("close", () => done());
    child.once("error", fail);
  });
  test.after(async () => { if (child.exitCode === null && child.signalCode === null) child.kill(); await closed; });
  assert.ok(child.pid);
  const pid = child.pid;
  assert.equal(processState(pid), "running");
  await assert.rejects(waitForProcessExit(pid, { timeout: 0 }), /not confirmed.*running/);
  await assert.rejects(waitForProcessExit(pid, { timeout: 0, observe: () => "unknown" }), /not confirmed.*unknown/);
  child.kill();
  await closed;
  await waitForProcessExit(pid, { timeout: 1000 });
});

it("reclaims a Windows job's marker only after every recorded PID is confirmed gone", async (test) => {
  const root = newDirectory(scratchDirectory(), "desktop-stale-foreground-");
  test.after(() => removeTree(root));
  const marker = nativePath(root, "foreground-unreleased");
  quarantineForeground(marker, "interrupted Windows batch", [100, 101, 102], true);
  for (const state of ["running", "unknown"] as const) {
    assert.throws(() => checkForeground(marker, (pid) => pid === 102 ? state : "exited"), /Owned PIDs: 100, 101, 102/);
    assert.equal(exists(marker), true);
  }
  checkForeground(marker, () => "exited");
  assert.equal(exists(marker), false);
  quarantineForeground(marker, "no native job owner found", [102]);
  assert.throws(() => checkForeground(marker, () => "exited"), /quarantined/);
});

it("fails a build interrupted after its root exits but while a descendant holds stdout", async (test) => {
  const root = newDirectory(scratchDirectory(), "desktop-interrupt-test-");
  test.after(() => removeTree(root));
  const pidFile = nativePath(root, "pid");
  const descendant = nativePath(root, "descendant");
  const holder = `require('node:fs').writeFileSync(process.argv[1], String(process.pid));
    setTimeout(() => {}, 5000);`;
  const launcher = `const { spawn } = require('node:child_process');
    const child = spawn(process.execPath, ['-e', process.argv[3], process.argv[2]], { stdio: 'inherit' });
    child.once('spawn', () => require('node:fs').writeFileSync(process.argv[1], String(process.pid)));
    child.unref();`;
  let finished = false;
  const step = runStep(nodeProgram, ["-e", launcher, pidFile, descendant, holder], { env: process.env, cwd: root, capture: true });
  void step.then(() => (finished = true), () => (finished = true));
  try {
    // The wrapper has to start and compile its job helper before the command exists.
    // Time the interrupted-after-exit transition only once its target PID is known.
    const launchDeadline = Date.now() + 30000;
    let deadline: number | undefined;
    for (;;) {
      const pid = Number(readTextIfExists(pidFile));
      if (pid > 0) {
        deadline ??= Date.now() + 4000;
        if (!processRunning(pid) && Number(readTextIfExists(descendant)) > 0) break;
      }
      assert.ok(Date.now() < (deadline ?? launchDeadline) && !finished, "the build command exited with its pipe still open");
      await delay(10);
    }
    assert.equal(finished, false, "the descendant still holds stdout");
    process.emit("SIGINT", "SIGINT");
    await assert.rejects(step, /was interrupted/);
    const pid = Number(readTextIfExists(descendant));
    const exitDeadline = Date.now() + 1000;
    while (processRunning(pid) && Date.now() < exitDeadline) await delay(10);
    assert.equal(processRunning(pid), false, "interruption stopped the descendant, not just its stdout");
  } finally {
    await step.catch(() => undefined);
  }
});


it("leaves no foreground marker if exit occurs during launcher initialization", async (test) => {
  const root = newDirectory(scratchDirectory(), "e2e-lease-before-launch-");
  test.after(() => removeTree(root));
  const marker = nativePath(root, "foreground");
  const owner = new BatchLifecycle(marker, "fixture apps");
  assert.equal(await owner.close(), true);
  assert.equal(exists(marker), false);
  assert.throws(() => owner.arm({}, async () => {}), /closed before the service started/);
  assert.doesNotThrow(() => new BatchLifecycle(marker, "next batch"));
});

it("joins signal and normal teardown and releases the lease after a failed stop is retried", async (test) => {
  const root = newDirectory(scratchDirectory(), "e2e-lease-retry-");
  test.after(() => removeTree(root));
  const marker = nativePath(root, "foreground");
  const owner = new BatchLifecycle(marker, "fixture apps");
  const service = {};
  let retries = 0;
  let release!: () => void;
  const stopped = new Promise<void>((resolve) => { release = resolve; });
  owner.arm(service, async () => { retries++; await stopped; });
  await assert.rejects(owner.stop(service, async () => { throw new Error("first stop failed"); }), /first stop failed/);
  const closing = owner.close();
  assert.equal(owner.close(), closing);
  assert.equal(exists(marker), true);
  release();
  assert.equal(await closing, true);
  assert.equal(retries, 1);
  assert.equal(owner.failures.length, 1);
  assert.equal(exists(marker), false);
});

it("retains the lease when service shutdown cannot be confirmed", async (test) => {
  const root = newDirectory(scratchDirectory(), "e2e-lease-unreleased-");
  test.after(() => removeTree(root));
  const marker = nativePath(root, "foreground");
  const owner = new BatchLifecycle(marker, "unreleased fixture apps");
  owner.arm({}, async () => { throw new Error("shutdown not confirmed"); });
  assert.equal(await owner.close(), false);
  assert.throws(() => new BatchLifecycle(marker, "next batch"), /unreleased fixture apps/);
});

it("keeps isolated build targets by default and copies binaries without removing a shared target", (test) => {
  const parent = newDirectory(scratchDirectory(), "desktop-target-test-");
  test.after(() => removeTree(parent));
  const isolated = buildFolder(parent);
  assert.equal(isolated.env.CARGO_TARGET_DIR, nativePath(isolated.root, "target"));
  assert.equal(isolated.env.CARGO_BUILD_BUILD_DIR, nativePath(isolated.root, "cargo-build"));
  removeTree(isolated.root);

  const shared = buildFolder(parent, { sharedTarget: true });
  const workspaceTarget = nativePath(workspace, "target");
  assert.equal(shared.env.CARGO_TARGET_DIR, workspaceTarget);
  assert.equal(shared.env.CARGO_BUILD_BUILD_DIR, workspaceTarget);

  const fakeTarget = newDirectory(parent, "fake-target-");
  const builtCli = nativePath(fakeTarget, "cli-bin");
  const builtApp = nativePath(fakeTarget, "app-bin");
  writeText(builtCli, "cli");
  writeText(builtApp, "app");

  const candidate = stageCandidateBinaries(shared.root, builtCli, builtApp);
  assert.equal(samePath(candidate.cli, builtCli), false);
  assert.equal(samePath(candidate.app, builtApp), false);
  assert.equal(samePath(candidate.app, nativePath(shared.root, ".", candidate.app.slice(shared.root.length + 1))), true);

  cleanup(candidate.root);
  assert.equal(exists(candidate.root), false);
  assert.equal(exists(builtCli), true);
  assert.equal(exists(builtApp), true);
});

it("reuses prepared process-tree resources across repeated steps and cleans them up", async () => {
  const dispose = prepareProcessTrees();
  try {
    const cwd = scratchDirectory();
    assert.equal(await runStep(nodeProgram, ["-e", "process.stdout.write('one')"], { env: process.env, cwd, capture: true }), "one");
    assert.equal(await runStep(nodeProgram, ["-e", "process.stdout.write('two')"], { env: process.env, cwd, capture: true }), "two");
  } finally {
    dispose();
  }
});
