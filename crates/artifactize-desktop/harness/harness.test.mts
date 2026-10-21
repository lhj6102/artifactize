// Exercise the actual Node tool protocol against a loopback peer; no native app mock.
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createHttpServer, type ServerResponse } from "node:http";
import { createServer, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";
import { it, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";
import { commandStream, forwardReadiness } from "./harness.mts";
import { embeddedPorts } from "../ui/e2e/platform.ts";

const harness = fileURLToPath(new URL("./harness.mts", import.meta.url));

type Envelope = { token: string; request: Record<string, unknown> };
async function peer(test: TestContext, reply: Record<string, unknown> | null) {
  const directory = mkdtempSync(join(tmpdir(), "artifactize-harness-protocol-"));
  const received: Envelope[] = [];
  const sockets = new Set<Socket>();
  const children = new Map<ReturnType<typeof execFile>, Promise<void>>();
  const server = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    let text = "";
    socket.on("data", (chunk) => {
      text += chunk;
      if (!text.includes("\n")) return;
      received.push(JSON.parse(text.trim()) as Envelope);
      socket.end(reply === null ? "" : JSON.stringify(reply) + "\n");
    });
  });
  test.after(async () => {
    try {
      for (const socket of sockets) socket.destroy();
      await Promise.all([...children].map(async ([child, closed]) => {
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
        await closed;
      }));
      if (server.listening) await new Promise<void>((done, fail) => server.close((error) => error ? fail(error) : done()));
    } finally { rmSync(directory, { recursive: true }); }
  });
  await new Promise<void>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => { server.off("error", fail); done(); });
  });
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  writeFileSync(join(directory, "control.json"), JSON.stringify({ port: address.port, token: "session-token" }), { mode: 0o600 });
  const call = (mode: string, args: Record<string, unknown> = {}) => new Promise<string>((done, fail) => {
    const child = execFile(process.execPath, [harness, mode], {
      env: { ...process.env, ARTIFACTIZE_SESSION_DIR: directory },
      encoding: "utf8", timeout: 15000,
    }, (error, stdout) => error ? fail(error) : done(stdout));
    children.set(child, new Promise<void>((done) => { child.once("close", () => done()); }));
    child.stdin!.end(JSON.stringify({ args }));
  });
  return { call, received };
}

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  for (const exitCode of [0, 7]) {
    it(`${signal} forwards control EOF and joins ${exitCode ? "failed" : "successful"} owner teardown before supervisor exit`, { timeout: 15000 }, async (test) => {
      let ownerStopping!: () => void;
      const stopping = new Promise<void>((done) => { ownerStopping = done; });
      const responses = new Set<ServerResponse>();
      let released = false;
      const release = () => {
        released = true;
        for (const response of responses) response.end("released");
      };
      // The fixture owns this release gate before spawning anything. Teardown can
      // open it even when owner readiness was never received or an assertion fails.
      const server = createHttpServer((_request, response) => {
        responses.add(response);
        response.once("close", () => responses.delete(response));
        ownerStopping();
        if (released) response.end("released");
      });
      let joinSupervisor = async () => {};
      test.after(async () => {
        release();
        try { await joinSupervisor(); }
        finally {
          if (server.listening) await new Promise<void>((done, fail) => server.close((error) => error ? fail(error) : done()));
        }
      });
      await new Promise<void>((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", () => { server.off("error", fail); done(); });
      });
      const address = server.address();
      assert.ok(address && typeof address !== "string");
      const owner = `
        import {get} from 'node:http';
        process.stdin.once('end', () => {
          get('http://127.0.0.1:${address.port}/release', (response) => {
            response.resume();
            response.once('end', () => process.exit(${exitCode}));
          }).once('error', (error) => { console.error(error); process.exit(1); });
        });
        process.stdin.resume();
        console.log('ready');
      `;
      const script = `import {superviseOwner} from ${JSON.stringify(new URL("./harness.mts", import.meta.url).href)};
        await superviseOwner({program: process.execPath, args: ['--input-type=module', '-e', ${JSON.stringify(owner)}], env: process.env});`;
      const supervisor = spawn(process.execPath, ["--input-type=module", "-e", script], { stdio: ["ignore", "pipe", "pipe"] });
      const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
        supervisor.once("error", fail);
        supervisor.once("close", (code, signal) => done({ code, signal }));
      });
      // Installed synchronously after spawn, before any acquisition can fail.
      joinSupervisor = async () => {
        try {
          if (supervisor.exitCode === null && supervisor.signalCode === null) supervisor.kill("SIGTERM");
          await closed;
        } finally { lines.close(); output.close(); }
      };
      let diagnostics = "";
      const lines = createInterface({ input: supervisor.stderr });
      lines.on("line", (line) => { diagnostics += line + "\n"; });
      const output = createInterface({ input: supervisor.stdout });
      const prematureExit = closed.then((status) => {
        throw new Error(`supervisor exited before owner release: ${JSON.stringify(status)}\n${diagnostics}`);
      });
      assert.deepEqual(await Promise.race([once(output, "line"), prematureExit]), ["ready"]);
      assert.equal(supervisor.kill(signal), true);
      await Promise.race([stopping, prematureExit]);
      assert.equal(supervisor.exitCode, null, "registered supervisor must remain until owner teardown finishes");
      assert.equal(supervisor.kill(signal), true, "repeated termination joins the same owner shutdown");
      release();
      const result = await closed;
      assert.deepEqual(result, { code: exitCode === 0 ? 0 : 1, signal: null });
      if (exitCode) assert.match(diagnostics, /desktop owner supervision failed/);
    });
  }
}

it("embedded endpoint configuration selects distinct loopback ports and releases them for the service", async () => {
  const ports = await embeddedPorts(3);
  assert.equal(new Set(ports).size, 3);
  for (const port of ports) {
    assert.ok(Number.isInteger(port) && port > 0 && port < 65536);
    const listener = createServer();
    try {
      await new Promise<void>((done, fail) => {
        listener.once("error", fail);
        listener.listen(port, "127.0.0.1", () => { listener.off("error", fail); done(); });
      });
    } finally {
      if (listener.listening) await new Promise<void>((done, fail) => listener.close((error) => error ? fail(error) : done()));
    }
  }
});

it("readiness is the only stdout line while all startup and later service logs remain diagnostics", async (test) => {
  const source = new PassThrough();
  const output = new PassThrough();
  const diagnostics = new PassThrough();
  let stdout = "";
  let stderr = "";
  output.on("data", (chunk) => { stdout += chunk; });
  diagnostics.on("data", (chunk) => { stderr += chunk; });
  const lines = forwardReadiness(source, output, diagnostics);
  test.after(() => { lines.close(); source.destroy(); output.destroy(); diagnostics.destroy(); });
  const closed = once(lines, "close");
  source.write("starting WDIO\nrea");
  assert.equal(stdout, "");
  source.end("dy\ncommand log\nready\nfinal log");
  await closed;
  output.end();
  diagnostics.end();
  assert.equal(stdout, "ready\n");
  assert.equal(stderr, "starting WDIO\ncommand log\nready\nfinal log\n");
});

it("every Agent action crosses the authenticated loopback protocol with its original arguments", async (test) => {
  const { call, received } = await peer(test, { text: "rendered window" });
  const actions = [
    { mode: "snapshot", args: {}, request: { op: "snapshot" } },
    { mode: "act", args: { action: "click", element: 3 }, request: { op: "click", element: 3 } },
    { mode: "act", args: { action: "type", element: 4, text: "한글 input" }, request: { op: "type", element: 4, text: "한글 input" } },
    { mode: "act", args: { action: "key", key: "Control+k" }, request: { op: "key", key: "Control+k" } },
    { mode: "launch", args: { screen: "monitor" }, request: { op: "launch", screen: "monitor" } },
    { mode: "launch", args: { screen: "repo" }, request: { op: "launch", screen: "repo" } },
    { mode: "launch", args: { screen: "review" }, request: { op: "launch", screen: "review" } },
    { mode: "launch", args: { screen: "graph" }, request: { op: "launch", screen: "graph" } },
    { mode: "change-declaration", args: {}, request: { op: "change-declaration" } },
  ];
  for (const action of actions) {
    assert.deepEqual(JSON.parse(await call(action.mode, action.args)), { content: [{ type: "text", text: "rendered window" }] });
    assert.deepEqual(received.at(-1), { token: "session-token", request: action.request });
  }
});

it("CLI preserves argv, exit status and both output streams", async (test) => {
  const { call, received } = await peer(test, { exitCode: 9, stdout: "stdout text", stderr: "stderr text" });
  assert.deepEqual(JSON.parse(await call("cli", { args: ["run", "list", "--json"] })), {
    isError: true, content: [{ type: "text", text: "exit 9\n--- stdout\nstdout text\n--- stderr\nstderr text" }],
  });
  assert.deepEqual(received, [{ token: "session-token", request: { op: "cli", args: ["run", "list", "--json"] } }]);
});

it("operation cleanup failures remain tool errors", async (test) => {
  const { call } = await peer(test, { error: "operation and cleanup failed: render failed; service failed" });
  assert.deepEqual(JSON.parse(await call("snapshot")), {
    isError: true, content: [{ type: "text", text: "operation and cleanup failed: render failed; service failed" }],
  });
});

it("stop requires explicit successful cleanup acknowledgement", async (test) => {
  const success = await peer(test, { stopped: true });
  assert.equal(await success.call("stop"), "");
  assert.deepEqual(success.received, [{ token: "session-token", request: { op: "stop" } }]);
  const missing = await peer(test, { text: "not stopped" });
  await assert.rejects(missing.call("stop"), /did not acknowledge cleanup/);
  const failed = await peer(test, { error: "service cleanup failed" });
  await assert.rejects(failed.call("stop"), /service cleanup failed/);
});

it("a dropped control connection never looks like a successful tool response", async (test) => {
  const { call } = await peer(test, null);
  await assert.rejects(call("snapshot"), /ended without answering/);
});

for (const fails of [false, true]) {
  it(`shutdown drains a ${fails ? "failed" : "successful"} relaunch and releases only its final session`, async () => {
    let resume!: () => void;
    const waiting = new Promise<void>((done) => { resume = done; });
    let started!: () => void;
    const active = new Promise<void>((done) => { started = done; });
    const events: string[] = [];
    let owner = "original";
    const commands = commandStream(() => { events.push("admission closed"); }, async () => {
      events.push(`service cleanup ${owner}`, "settle Runs", "remove inputs");
    });
    const launch = commands.run(async () => {
      started();
      await waiting;
      owner = "replacement";
      if (fails) throw new Error("render failed");
    });
    await active;
    const queued = assert.rejects(commands.run(async () => { events.push("unexpected queued work"); }), /session is stopping/);
    const ending = commands.end();
    assert.equal(commands.end(), ending);
    assert.deepEqual(events, ["admission closed"]);
    await assert.rejects(commands.run(async () => {}), /session is stopping/);
    const completed = fails ? assert.rejects(launch, /render failed/) : launch;
    resume();
    await Promise.all([completed, queued, ending]);
    assert.deepEqual(events, ["admission closed", "service cleanup replacement", "settle Runs", "remove inputs"]);
  });
}

it("a failed service release cannot proceed to fixture deletion or retry ownership", async () => {
  const events: string[] = [];
  const commands = commandStream(() => {}, async () => {
    events.push("service cleanup");
    throw new Error("cleanup failed");
  });
  const ending = commands.end();
  await assert.rejects(ending, /cleanup failed/);
  assert.equal(commands.end(), ending);
  await assert.rejects(commands.run(async () => { events.push("remove inputs"); }), /session is stopping/);
  assert.deepEqual(events, ["service cleanup"]);
});
