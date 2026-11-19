// Exercise the public Launcher and actual Tauri service, not a replacement service hook.
import assert from "node:assert/strict";
import { it } from "node:test";
import { observe } from "./domain-fixtures/observe.ts";
import { joinGatedTools, releaseToolGates, slowTool } from "./process-fixtures.ts";
import { environment } from "./environment.ts";
import {
  childDirectories, executableName, fixtureRunner, makeDirectory, nativePath, newDirectory, removeTree, runnerCommand, scratchDirectory,
  exists, nodeProgram, processRunning, readLoopback, runProgramOk, runStep, serveLoopback, suitePath, writeJson, writeText, type LoopbackServer, type NativePath,
} from "./platform.ts";

function root(): NativePath {
  return newDirectory(scratchDirectory(), "desktop-startup-test-");
}

function fixture(root: NativePath) {
  return { root: nativePath(root, "owner"), repo: nativePath(root, "owner", "repo"), state: nativePath(root, "owner", "state") };
}

async function launch(root: NativePath, id: string, signal: AbortSignal, output = `${id}-logs`) {
  const env = {
    ...environment(fixture(root)),
    DESKTOP_TEST_CANDIDATE: nativePath(root, "candidate.json"),
    DESKTOP_TEST_BATCH: JSON.stringify([id]),
    DESKTOP_TEST_BATCH_OUTPUT: nativePath(root, output),
  };
  const command = runnerCommand([suitePath("run.ts"), "batch"], env);
  return await fixtureRunner(command, signal);
}

it("a failed preparation stops the real service before its executable boundary", { timeout: 120000 }, async (test) => {
  const directory = root();
  try {
    // A declared invalid executable makes a service launch attempt an immediate,
    // observable error without starting any native app or WebDriver server.
    const app = nativePath(directory, executableName("unlaunchable-app"));
    writeText(app, "not an executable", { private: true });
    writeJson(nativePath(directory, "candidate.json"), { root: directory, cli: app, app, observer: app, manifest: nativePath(directory, "candidate.json") });
    // Fail on a private external input after setup has acquired its HTTP fixture.
    // Never occupy the product's fixed auth callback port: the suite may run beside us.
    const owner = nativePath(directory, "runner-environment");
    writeText(owner, "not a directory");
    const blocked = await launch(directory, "settings-keys", test.signal);
    assert.equal(blocked.code, 1, blocked.stdout + blocked.stderr);
    assert.match(blocked.stderr, /Desktop scenario preparation failed/);
    assert.doesNotMatch(blocked.stderr, /Failed to start embedded WebDriver/);
    assert.match(blocked.stderr, /\b(?:ENOTDIR|EEXIST|EACCES|EPERM)\b/);
    assert.ok(blocked.stderr.includes("runner-environment"), "nested causes identify the blocked owner path");
    assert.deepEqual(childDirectories(nativePath(directory, "fixtures")), [], "failed preparation released its fixture inputs");

    // Positive control: fixing only that input must reach the same real service's
    // executable boundary, proving the first rejection was before app admission.
    removeTree(owner);
    const admitted = await launch(directory, "settings-keys", test.signal, "settings-keys-retry-logs");
    assert.equal(admitted.code, 1, admitted.stdout + admitted.stderr);
    assert.match(admitted.stderr, /Failed to start embedded WebDriver/);
    assert.match(admitted.stderr, /EACCES|EINVAL|ENOEXEC|spawn/);
    assert.doesNotMatch(admitted.stderr, /Desktop scenario preparation failed/);
  } finally {
    removeTree(directory);
  }
});

it("a completed build step returns its captured output", { timeout: 120000 }, async (test) => {
  const directory = root();
  try {
    const stdout = await runStep(nodeProgram, ["-e", "process.stdout.write('build complete')"], {
      env: environment(fixture(directory)), cwd: directory, capture: true, signal: test.signal,
    });
    assert.equal(stdout, "build complete");
  } finally { removeTree(directory); }
});

it("cancelling a wrapped build step joins its descendants before releasing inputs", { timeout: 120000 }, async (test) => {
  const directory = root();
  const cancellation = new AbortController();
  const reason = new Error("fixture owner cancelled the runner");
  const servers: LoopbackServer[] = [];
  let running: Promise<{ error?: unknown }> | undefined;
  try {
    let received: (port: number) => void = () => {};
    let refused: (error: unknown) => void = () => {};
    const ready = new Promise<number>((done, fail) => { received = done; refused = fail; });
    const receiver = await serveLoopback((socket) => {
      let input = "";
      socket.setEncoding("utf8");
      socket.on("data", (chunk: string) => { input += chunk; });
      socket.once("error", refused);
      socket.once("end", () => {
        try { received(JSON.parse(input) as number); } catch (error) { refused(error); }
      });
    });
    servers.push(receiver);
    const port = receiver.port;
    const command = runnerCommand([suitePath("domain-fixtures", "tree.ts"), "owner", String(port)], environment(fixture(directory)));
    running = runStep(command.program, command.args, {
      env: command.env, cwd: directory, capture: true, signal: AbortSignal.any([test.signal, cancellation.signal]),
    }).then(() => ({}), (error: unknown) => ({ error }));
    const descendant = await Promise.race([ready, running.then(({ error }) => {
      throw error ?? new Error("runner exited before its descendant reported its process id");
    })]);
    assert.ok(processRunning(descendant), "the descendant is running before cancellation");
    cancellation.abort(reason);
    assert.equal((await running).error, reason, "cancellation is a failure, never a passing runner result");
    // A read-only lifetime check of the reported process is independent evidence that the
    // descendant exited, not just that its parent reported an abort or accepted a kill.
    assert.equal(processRunning(descendant), false, "the runner joined its descendant before returning");
  } finally {
    cancellation.abort(reason);
    await running;
    const released = await Promise.allSettled(servers.map((server) => server.close()));
    removeTree(directory);
    const failures = released.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
    if (failures.length) throw new AggregateError(failures, "runner regression fixture release failed");
  }
});

it("joins fixture descendants even after their root process returned", { timeout: 120000 }, async (test) => {
  const directory = root();
  let descendant: number | undefined;
  let receiver: LoopbackServer | undefined;
  try {
    receiver = await serveLoopback((socket) => {
      let input = "";
      socket.setEncoding("utf8");
      socket.on("data", (chunk: string) => { input += chunk; });
      socket.once("end", () => { descendant = JSON.parse(input) as number; });
    });
    const command = runnerCommand([suitePath("domain-fixtures", "tree.ts"), "owner-exits", String(receiver.port)], environment(fixture(directory)));
    const result = await fixtureRunner(command, test.signal);
    assert.equal(result.code, 0, result.stderr);
    await observe(() => descendant !== undefined, () => "the receiver did not deliver the descendant PID");
    assert.ok(descendant && !processRunning(descendant), "the root returned but the same fixture had to join its still-live descendant");
  } finally {
    try { await receiver?.close(); } finally { removeTree(directory); }
  }
});

it("the cleanup entry point retains inputs if native shutdown is unconfirmed", { timeout: 120000 }, async (test) => {
  const directory = root();
  try {
    const manifest = nativePath(directory, "candidate.json");
    const app = nativePath(directory, executableName("unconfirmed-app"));
    writeText(app, "the app still may use this");
    writeJson(manifest, { root: directory, cli: app, app, observer: app, manifest });
    const journal = makeDirectory(nativePath(directory, "admissions", "unconfirmed"));
    const runner = Number(runProgramOk(nodeProgram, ["-p", "process.pid"]));
    writeJson(nativePath(journal, "expected.json"), { runner, names: ["unconfirmed.owner.json"] });
    const command = { program: nodeProgram, args: [suitePath("run.ts"), "cleanup", manifest], env: environment(fixture(directory)) };
    const refused = await fixtureRunner(command, test.signal);
    assert.equal(refused.code, 1);
    assert.match(refused.stderr, /birth record missing/);
    assert.equal(exists(app), true);
    // Positive control: removing the unconfirmed admission, not changing the fixture,
    // lets the same public cleanup command delete it.
    removeTree(journal);
    const released = await fixtureRunner(command, test.signal);
    assert.equal(released.code, 0, released.stderr);
    assert.equal(exists(directory), false);
  } finally { removeTree(directory); }
});

it("a stand-in starting after gate teardown cannot recreate or wait on its released fixture", { timeout: 10000 }, () => {
  const directory = root();
  const owned = fixture(directory);
  try {
    makeDirectory(nativePath(owned.root, "gated-tools"));
    releaseToolGates(owned);
    // A late process still has its literal command line, but no open admission.
    const args = [suitePath("domain-fixtures", "command.ts"), "talk", nativePath(owned.root, "talk.gate")];
    assert.equal(runProgramOk(nodeProgram, args, { timeout: 5000 }), "");
    assert.equal(exists(nativePath(owned.root, "gated-tools")), false);
    removeTree(owned.root);
    assert.equal(runProgramOk(nodeProgram, args, { timeout: 5000 }), "");
    assert.equal(exists(owned.root), false);
  } finally { removeTree(directory); }
});

it("fixture teardown joins a slow tool without app cancellation", { timeout: 10000 }, async (test) => {
  const directory = root();
  const cancellation = new AbortController();
  let running: ReturnType<typeof fixtureRunner> | undefined;
  try {
    // Setup is inside the cleanup scope, so its failure still removes the root.
    const owned = fixture(directory);
    makeDirectory(nativePath(owned.root, "gated-tools"));
    const command = { program: nodeProgram, args: [suitePath("domain-fixtures", "command.ts"), "slow", nativePath(owned.root, "slow.endpoint")],
      env: environment(owned) };
    running = fixtureRunner(command, AbortSignal.any([cancellation.signal, test.signal]));
    await observe(async () => await slowTool(owned)?.() === true, () => "the slow tool did not serve its owned endpoint");
    const alive = slowTool(owned)!;
    releaseToolGates(owned);
    const result = await running;
    assert.equal(result.code, 0, result.stderr);
    await joinGatedTools(owned);
    assert.equal(await alive(), false, "the same endpoint is gone after the fixture gate closes");
  } finally {
    cancellation.abort();
    try { await running; } finally { removeTree(directory); }
  }
});

it("closing an owned TCP endpoint joins a stalled connection and repeated callers", { timeout: 10000 }, async (test) => {
  let connected!: () => void;
  const accepted = new Promise<void>((done) => { connected = done; });
  const server = await serveLoopback(() => connected());
  const client = readLoopback(server.port, 5000);
  let cancelled: () => void = () => {};
  const interrupted = new Promise<never>((_done, fail) => {
    cancelled = () => fail(test.signal.reason);
    if (test.signal.aborted) cancelled();
    else test.signal.addEventListener("abort", cancelled, { once: true });
  });
  try {
    await Promise.race([accepted, interrupted, client.then(() => { throw new Error("the client disconnected before acceptance"); })]);
    const checkClosed = async (closing: Promise<void>) => {
      await closing;
      assert.equal(server.listening(), false);
      assert.ok([null, ""].includes(await client), "the stalled accepted peer was disconnected");
    };
    await Promise.all([checkClosed(server.close()), checkClosed(server.close())]);
  } finally {
    test.signal.removeEventListener("abort", cancelled);
    await server.close();
  }
});
