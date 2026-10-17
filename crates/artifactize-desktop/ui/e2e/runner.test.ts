// Exercise the public Launcher and actual Tauri service, not a replacement service hook.
import assert from "node:assert/strict";
import { it } from "node:test";
import { environment } from "./environment.ts";
import {
  executableName, fixtureRunner, makeDirectory, nativePath, newDirectory, removeTree, runnerCommand, scratchDirectory,
  processRunning, serveLoopback, suitePath, writeJson, writeText, type LoopbackServer, type NativePath,
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
    const context = makeDirectory(nativePath(directory, "settings-remote-logs", "settings-remote.json"));
    const blocked = await launch(directory, "settings-remote", test.signal);
    assert.equal(blocked.code, 1, blocked.stdout + blocked.stderr);
    assert.match(blocked.stderr, /Desktop scenario preparation failed/);
    assert.doesNotMatch(blocked.stderr, /Failed to start embedded WebDriver/);
    assert.match(blocked.stderr, /\b(?:EISDIR|EACCES|EPERM)\b/);
    assert.ok(blocked.stderr.includes(context), "nested causes identify this fixture's failed context output");

    // Positive control: fixing only that input must reach the same real service's
    // executable boundary, proving the first rejection was before app admission.
    removeTree(context);
    const admitted = await launch(directory, "settings-remote", test.signal, "settings-remote-retry-logs");
    assert.equal(admitted.code, 1, admitted.stdout + admitted.stderr);
    assert.match(admitted.stderr, /Failed to start embedded WebDriver/);
    assert.match(admitted.stderr, /EACCES|EINVAL|ENOEXEC|spawn/);
    assert.doesNotMatch(admitted.stderr, /Desktop scenario preparation failed/);
  } finally {
    removeTree(directory);
  }
});

it("cancelling a wrapped runner joins its fixture descendants before releasing inputs", { timeout: 120000 }, async (test) => {
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
    running = fixtureRunner(command, AbortSignal.any([test.signal, cancellation.signal]))
      .then(() => ({}), (error: unknown) => ({ error }));
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
