// Exercise the public Launcher and actual Tauri service, not a replacement service hook.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { it } from "node:test";
import { environment } from "./environment.ts";
import { executableSuffix, fixtureRunner, runnerCommand } from "./platform.ts";

function root(): string {
  const parent = process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir();
  mkdirSync(parent, { recursive: true });
  return mkdtempSync(join(parent, "desktop-startup-test-"));
}

function fixture(root: string) {
  return { root: join(root, "owner"), repo: join(root, "owner", "repo"), state: join(root, "owner", "state") };
}

async function launch(root: string, id: string, signal: AbortSignal) {
  const env = {
    ...environment(fixture(root)),
    DESKTOP_TEST_CANDIDATE: join(root, "candidate.json"),
    DESKTOP_TEST_BATCH: JSON.stringify([id]),
    DESKTOP_TEST_BATCH_OUTPUT: join(root, `${id}-logs`),
  };
  const command = runnerCommand([join(import.meta.dirname, "run.ts"), "batch"], env);
  return await fixtureRunner(command, signal);
}

it("a failed preparation stops the real service before its executable boundary", { timeout: 120000 }, async (test) => {
  const directory = root();
  try {
    // A declared invalid executable makes a service launch attempt an immediate,
    // observable error without starting any native app or WebDriver server.
    const app = join(directory, `unlaunchable-app${executableSuffix}`);
    writeFileSync(app, "not an executable", { mode: 0o600 });
    writeFileSync(join(directory, "candidate.json"), JSON.stringify({ root: directory, cli: app, app, observer: app, manifest: join(directory, "candidate.json") }));
    // Fail on a private external input after setup has acquired its HTTP fixture.
    // Never occupy the product's fixed auth callback port: the suite may run beside us.
    const context = join(directory, "settings-remote-logs", "settings-remote.json");
    mkdirSync(context, { recursive: true });
    const blocked = await launch(directory, "settings-remote", test.signal);
    assert.equal(blocked.code, 1, blocked.stdout + blocked.stderr);
    assert.match(blocked.stderr, /Desktop scenario preparation failed/);
    assert.doesNotMatch(blocked.stderr, /Failed to start embedded WebDriver/);
    assert.match(blocked.stderr, /\b(?:EISDIR|EACCES|EPERM)\b/);
    assert.ok(blocked.stderr.includes(context), "nested causes identify this fixture's failed context output");

    // Positive control: fixing only that input must reach the same real service's
    // executable boundary, proving the first rejection was before app admission.
    rmSync(context, { recursive: true });
    const admitted = await launch(directory, "settings-remote", test.signal);
    assert.equal(admitted.code, 1, admitted.stdout + admitted.stderr);
    assert.match(admitted.stderr, /Failed to start embedded WebDriver/);
    assert.match(admitted.stderr, /EACCES|EINVAL|ENOEXEC|spawn/);
    assert.doesNotMatch(admitted.stderr, /Desktop scenario preparation failed/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

function listen(server: Server, port = 0): Promise<number> {
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      assert.ok(address && typeof address === "object");
      done(address.port);
    });
  });
}

function close(server: Server): Promise<void> {
  if (!server.listening) return Promise.resolve();
  return new Promise((done, fail) => server.close((error) => error ? fail(error) : done()));
}

it("cancelling a wrapped runner joins its fixture descendants before releasing inputs", { timeout: 120000 }, async (test) => {
  const directory = root();
  const cancellation = new AbortController();
  const reason = new Error("fixture owner cancelled the runner");
  const receiver = createServer();
  const probe = createServer();
  let running: Promise<{ error?: unknown }> | undefined;
  try {
    const port = await listen(receiver);
    const ready = new Promise<number>((done, fail) => {
      receiver.once("connection", (socket) => {
        let input = "";
        socket.setEncoding("utf8");
        socket.on("data", (chunk: string) => { input += chunk; });
        socket.once("error", fail);
        socket.once("end", () => {
          try { done(JSON.parse(input) as number); } catch (error) { fail(error); }
        });
      });
    });
    const descendant = `
import { createConnection, createServer } from "node:net";
const server = createServer();
server.listen(0, "127.0.0.1", () => {
  const connection = createConnection({ host: "127.0.0.1", port: Number(process.argv[1]) });
  connection.on("connect", () => connection.end(JSON.stringify(server.address().port)));
});
`;
    const owner = `
import { spawn } from "node:child_process";
const child = spawn(process.execPath, ["--input-type=module", "-e", ${JSON.stringify(descendant)}, process.argv[1]], { stdio: "inherit" });
child.on("error", (error) => { throw error; });
`;
    const command = runnerCommand(["--input-type=module", "-e", owner, String(port)], environment(fixture(directory)));
    running = fixtureRunner(command, AbortSignal.any([test.signal, cancellation.signal]))
      .then(() => ({}), (error: unknown) => ({ error }));
    const endpoint = await Promise.race([ready, running.then(({ error }) => {
      throw error ?? new Error("runner exited before its descendant acquired the fixture endpoint");
    })]);
    cancellation.abort(reason);
    assert.equal((await running).error, reason, "cancellation is a failure, never a passing runner result");
    // Rebinding the observed endpoint is independent evidence that the descendant
    // exited, not just that its parent reported an abort or accepted a kill signal.
    assert.equal(await listen(probe, endpoint), endpoint);
  } finally {
    cancellation.abort(reason);
    await running;
    const released = await Promise.allSettled([close(receiver), close(probe)]);
    rmSync(directory, { recursive: true, force: true });
    const failures = released.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
    if (failures.length) throw new AggregateError(failures, "runner regression fixture release failed");
  }
});
