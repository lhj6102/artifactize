// The runner's own parts: batching, fixture environments, teardown and build steps.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { it } from "node:test";
import { runStep } from "./build.ts";
import { own } from "./fixtures/owned.ts";
import { environment } from "./fixtures/repo.ts";
import { nodeProgram } from "./platform.ts";
import { exists, nativePath, removeTree, scratchDirectory, suitePath, newDirectory } from "./portable.ts";
import { batches } from "./run.ts";

it("batches scenarios in order, at most `size` apps at once, one that runs alone by itself", () => {
  const ids = (names: string) => names.split(" ").map((id) => ({ id, alone: id.startsWith("x") }));
  assert.deepEqual(batches(ids("a b c d e"), 2), [["a", "b"], ["c", "d"], ["e"]]);
  assert.deepEqual(batches(ids("a x1 b c x2"), 2), [["a"], ["x1"], ["b", "c"], ["x2"]]);
  assert.deepEqual(batches(ids("a b c"), 1), [["a"], ["b"], ["c"]]);
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

it("teardown runs every cleanup, last first, ends a stand-in waiting for its gate and deletes the folder", async () => {
  const scope = own(nativePath(scratchDirectory(), "desktop-owned-test"));
  const order: string[] = [];
  scope.defer(() => { order.push("first"); });
  scope.defer(() => { throw new Error("a cleanup that fails"); });
  scope.defer(async () => { order.push("last"); });
  // A stand-in tool that waits for a gate nobody opens.
  const waiting = spawn(nodeProgram, [suitePath("fixtures", "program.ts"), "slow", nativePath(scope.root, "slow.gate")], { stdio: "ignore" });
  const ended = new Promise<number | null>((done) => waiting.once("exit", (code) => done(code)));
  await assert.rejects(scope.close(), (error: AggregateError) => error.errors.length === 1 && /a cleanup that fails/.test(String(error.errors[0])));
  assert.deepEqual(order, ["last", "first"]);
  assert.equal(await ended, 0, "the stand-in ended once its fixture closed");
  assert.equal(exists(scope.root), false);
  removeTree(nativePath(scratchDirectory(), "desktop-owned-test"));
});

it("returns a build step's output and fails a step that fails", async () => {
  const cwd = scratchDirectory();
  assert.equal(await runStep(nodeProgram, ["-e", "process.stdout.write('built')"], { env: process.env, cwd, capture: true }), "built");
  await assert.rejects(runStep(nodeProgram, ["-e", "process.exit(3)"], { env: process.env, cwd }), /exited 3/);
});
