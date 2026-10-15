// Linux WebKitGTK's OS fixture boundary: saved terminal state is not process exit,
// and a reused PID must not hold cleanup hostage. No app or backend is replaced here.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { describe, it } from "node:test";
import { cleanup } from "./build.ts";
import { exited, finishFixture, removeFixture, running, workers } from "./runs.ts";
import { groupAlive, Owned, registerGroup } from "./owned.ts";

describe("Linux durable worker exit", { skip: process.platform !== "linux" && "WebKitGTK's native fixture layer observes procfs birth identities" }, () => {
  it("distinguishes an actual worker, another birth identity and an exited worker", async () => {
    const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-run-exit-"));
    // Its stdin is an ownership lease: even a killed test runner closes it and ends it.
    const child = spawn(process.execPath, ["--input-type=module", "-e",
      'process.stdin.resume(); process.stdin.on("end", () => process.exit(0)); console.log("ready");',
      "--", "--state-dir", root, "__execute", "run-exit-probe"], { stdio: ["pipe", "pipe", "inherit"] });
    const ended = new Promise<void>((done) => child.once("close", () => done()));
    try {
      await new Promise<void>((done, fail) => {
        const lines = createInterface({ input: child.stdout });
        const deadline = setTimeout(() => { lines.close(); fail(new Error("worker probe never became ready")); }, 5000);
        lines.once("line", () => { clearTimeout(deadline); lines.close(); done(); });
        child.once("error", fail);
      });
      const owners = workers(root);
      assert.equal(owners.length, 1);
      const owner = owners[0];
      assert.equal(owner.pid, child.pid);
      assert.equal(running(owner), true);
      assert.equal(running({ ...owner, start_time: owner.start_time + 1 }), false, "a reused PID is not the owned lifetime");
      assert.throws(() => exited(owners, 0), /preserve their working files/, "terminal publication alone cannot make a live worker exited");
      child.stdin.end();
      await ended;
      assert.equal(running(owner), false);
      exited(owners, 0);
    } finally {
      child.kill("SIGKILL");
      await ended;
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("removes a fixture on normal completion and assertion unwind only after shutdown", async () => {
    for (const unwind of [false, true]) {
      const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-fixture-finish-"));
      writeFileSync(join(root, "input"), "owned input");
      let stopped = false;
      const failure = new Error("journey assertion failed");
      try {
        const exercise = async () => {
          try { if (unwind) throw failure; }
          finally {
            await finishFixture(process.execPath, root, async () => {
              assert.ok(existsSync(join(root, "input")), "inputs exist until the shutdown boundary");
              stopped = true;
            });
          }
        };
        if (unwind) await assert.rejects(exercise(), (error: unknown) => error === failure);
        else await exercise();
        assert.ok(stopped);
        assert.equal(existsSync(root), false);
      } finally { rmSync(root, { recursive: true, force: true }); }
    }
  });

  it("preserves inputs and the original error when native shutdown fails", async () => {
    const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-fixture-stop-failure-"));
    const failure = new Error("native shutdown failed");
    try {
      writeFileSync(join(root, "input"), "owned input");
      await assert.rejects(finishFixture(process.execPath, root, async () => { throw failure; }),
        (error: unknown) => error === failure);
      assert.ok(existsSync(join(root, "input")));
    } finally { rmSync(root, { recursive: true, force: true }); }
  });

  it("preserves native inputs until the actual owned group exits, without following another birth", async () => {
    const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-fixture-group-"));
    const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e",
      'process.stdin.resume(); console.log("ready");'], { stdio: ["pipe", "pipe", "inherit"] });
    try {
      await new Promise<void>((done, fail) => {
        const lines = createInterface({ input: owner.child.stdout! });
        const deadline = setTimeout(() => { lines.close(); fail(new Error("native group probe never became ready")); }, 5000);
        lines.once("line", () => { clearTimeout(deadline); lines.close(); done(); });
        owner.child.once("error", fail);
      });
      writeFileSync(join(root, "input"), "owned profile");
      registerGroup(root, owner.group);
      assert.equal(groupAlive(owner.group, -1), false, "another birth identity is not this lifetime");
      assert.throws(() => removeFixture(process.execPath, root), /owned native group.*preserve fixture/);
      assert.ok(existsSync(join(root, "input")), "a live writer retains its profile");
      await owner.stop();
      removeFixture(process.execPath, root);
      assert.equal(existsSync(root), false);
    } finally {
      await owner.stop();
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("preserves a nested executable snapshot until its owner has cleaned it", () => {
    const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-fixture-nested-"));
    try {
      mkdirSync(join(root, "session", "candidate-owned"), { recursive: true });
      assert.throws(() => removeFixture(process.execPath, root), /owned executable snapshots.*preserve fixture/);
      assert.ok(existsSync(join(root, "session", "candidate-owned")));
    } finally { rmSync(root, { recursive: true, force: true }); }
  });

  it("preserves state and executable inputs when durable-Run cleanup fails", async () => {
    const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-run-failure-"));
    try {
      mkdirSync(join(root, "tmp"));
      writeFileSync(join(root, "groups"), "");
      const cli = join(root, "cli");
      // A declared executable that refuses artifactize's arguments creates a real CLI
      // failure at the cleanup seam, without a fake saved-terminal JSON response.
      symlinkSync(process.execPath, cli);
      writeFileSync(join(root, "candidate.json"), JSON.stringify({ root, cli, app: cli }));
      const state = join(root, "fixtures", "fixture-failure", "graph-state");
      mkdirSync(state, { recursive: true });
      mkdirSync(join(root, "fixtures", "fixture-failure", "repo"));
      writeFileSync(join(state, "state.sqlite"), "owned state");
      await assert.rejects(cleanup(root), /cannot list fixture Runs|bad option/);
      let stopped = false;
      await assert.rejects(finishFixture(cli, join(root, "fixtures", "fixture-failure"), async () => { stopped = true; }),
        /cannot list fixture Runs|bad option/);
      assert.equal(stopped, true, "settlement failure still shuts down the holder on unwind");
      const stopError = new Error("native stop also failed");
      await assert.rejects(finishFixture(cli, join(root, "fixtures", "fixture-failure"), async () => { throw stopError; }),
        (error: unknown) => error instanceof AggregateError && error.errors.length === 2 && error.errors[1] === stopError);
      assert.ok(existsSync(join(state, "state.sqlite")), "failure retains owned state");
      assert.ok(existsSync(cli), "failure retains executable inputs");
    } finally { rmSync(root, { recursive: true, force: true }); }
  });
});
