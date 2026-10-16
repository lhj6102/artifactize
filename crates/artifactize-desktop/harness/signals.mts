// Real registered-start signal seams. Invoke with an explicit prepared candidate and
// DESKTOP_TEST_SIGNAL_OUTPUT under the caller's private scratch; never builds an app.
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { Socket } from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { readCandidate } from "../ui/e2e/build.ts";

const COMPLETION_TIMEOUT = 120000; // A watchdog, never evidence that a state was reached.
const CONNECTION_TIMEOUT = 2000; // A live but unresponsive listener must fail, not pass.

async function bounded<T>(operation: Promise<T>, label: string): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_done, fail) => {
      timeout = setTimeout(() => fail(new Error(`${label} did not complete`)), COMPLETION_TIMEOUT);
    })]);
  } finally { clearTimeout(timeout); }
}

async function listenerClosed(port: number): Promise<void> {
  const socket = new Socket();
  try {
    await assert.rejects(new Promise<void>((done, fail) => {
      socket.once("error", fail);
      socket.once("connect", done);
      socket.setTimeout(CONNECTION_TIMEOUT, () => socket.destroy(new Error(`listener ${port} did not answer`)));
      socket.connect(port, "127.0.0.1");
    }), (error: unknown) => {
      const failure = error as NodeJS.ErrnoException & { address?: string; port?: number };
      return failure.code === "ECONNREFUSED" && failure.syscall === "connect" && failure.address === "127.0.0.1" && failure.port === port;
    }, "the known service listener must refuse connections after registered wrapper exit");
  } finally { socket.destroy(); }
}

export async function verifySignals(manifest: string, output: string): Promise<void> {
  readCandidate(manifest);
  mkdirSync(output, { recursive: true });
  const harnessUrl = new URL("./harness.mts", import.meta.url);
  const harness = fileURLToPath(harnessUrl);
  // This private entry uses the same supervisor and platform wrapper as start.
  // The callback holds actual Browser acquisition before readiness, until the
  // supervisor acknowledges termination by aborting and joining the callback.
  const heldStartup = `
    import {superviseOwner} from ${JSON.stringify(harnessUrl.href)};
    import {runnerCommand} from ${JSON.stringify(new URL("../ui/e2e/platform.ts", import.meta.url).href)};
    await superviseOwner(runnerCommand([${JSON.stringify(harness)}, 'serve']), async (shutdown) => {
      console.error('STARTUP_GATE');
      await new Promise((done) => {
        if (shutdown.aborted) done();
        else shutdown.addEventListener('abort', () => done(), {once: true});
      });
    });
  `;
  for (const phase of ["startup", "ready"] as const) {
    for (const signal of ["SIGINT", "SIGTERM"] as const) {
      const directory = mkdtempSync(join(output, `${phase}-${signal}-`));
      const env = { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, DESKTOP_TEST_CANDIDATE: manifest };
      const args = phase === "startup" ? ["--input-type=module", "-e", heldStartup] : [harness, "start"];
      const registered = spawn(process.execPath, args, { env, stdio: ["ignore", "pipe", "pipe"] });
      const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
        registered.once("error", fail);
        registered.once("close", (code, signal) => done({ code, signal }));
      });
      // Own shutdown and join immediately after spawn, not after acquiring a
      // readiness marker, endpoint, Browser, or cancellation acknowledgement.
      try {
        let acquired!: () => void;
        const boundary = new Promise<void>((done) => { acquired = done; });
        let acknowledged!: () => void;
        const cancellation = new Promise<void>((done) => { acknowledged = done; });
        let stdout = "";
        let diagnostics = "";
        const ports = new Set<number>();
        const record = (file: string, text: string) => writeFileSync(join(directory, file), text);
        registered.stdout.on("data", (chunk: Buffer) => {
          stdout += chunk.toString();
          record("stdout.log", stdout);
          if (phase === "ready" && stdout.split("\n").includes("ready")) acquired();
        });
        registered.stderr.on("data", (chunk: Buffer) => {
          diagnostics += chunk.toString();
          record("stderr.log", diagnostics);
          for (const match of diagnostics.matchAll(/WebDriver server ready on port (\d+)/g)) ports.add(Number(match[1]));
          const lines = diagnostics.split("\n");
          if (phase === "startup" && lines.includes("STARTUP_GATE")) acquired();
          if (lines.includes(`harness:shutdown-requested ${phase}`)) acknowledged();
        });
        const beforeExit = closed.then((status) => {
          throw new Error(`registered start exited before the expected state: ${JSON.stringify(status)}\n${diagnostics}`);
        });
        const invoke = (mode: string, args: Record<string, unknown> = {}) => new Promise<string>((done, fail) => {
          const command = execFile(process.execPath, [harness, mode], { env, encoding: "utf8", timeout: 60000 },
            (error, stdout) => error ? fail(error) : done(stdout));
          command.stdin!.end(JSON.stringify({ args }));
        });
        await bounded(Promise.race([boundary, beforeExit]), `${phase} acquisition`);
        assert.equal(existsSync(join(directory, "control.json")), phase === "ready");
        if (phase === "ready") {
          const launch = JSON.parse(await invoke("launch", { screen: "graph" })) as { isError?: boolean };
          assert.equal(launch.isError, undefined);
          const submission = JSON.parse(await invoke("cli", { args: ["verify", "--all", "--json"] })) as {
            isError?: boolean; content: { text: string }[];
          };
          assert.equal(submission.isError, undefined);
          assert.match(submission.content[0].text, /"run":\s*"run-/, "a real durable Run exists when termination arrives");
        }
        assert.equal(registered.kill(signal), true, "signal targets only the registered start wrapper");
        // Receipt of cancellation is not cleanup completion. In startup this ACK
        // also proves shutdown was latched while the before-ready gate was held.
        await bounded(Promise.race([cancellation, beforeExit]), `${phase} ${signal} acknowledgement`);
        const status = await bounded(closed, `${phase} ${signal} cleanup`);
        assert.deepEqual(status, { code: 0, signal: null }, diagnostics);
        assert.equal(stdout, phase === "startup" ? "" : "ready\n", "cancelled startup must never announce readiness");
        assert.equal(existsSync(join(directory, "control.json")), false);
        assert.deepEqual(readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isDirectory()), [],
          "service and public Run cleanup must finish before candidate/fixture removal");
        assert.ok(ports.size > 0, "the real service actually started its embedded listener");
        for (const port of ports) await listenerClosed(port);
        record("result.json", JSON.stringify({ phase, signal, cancellationAcknowledged: true, status, ports: [...ports], inputsRemoved: true }, null, 2));
        console.log(`${phase} ${signal}: GREEN; cancellation acknowledged, standard owner joined, app listener refused connections, durable work and inputs released`);
      } finally {
        // Failed acquisition/assertion uses the same registered-owner shutdown;
        // native apps/drivers/displays and their inputs remain service-owned.
        if (registered.exitCode === null && registered.signalCode === null) registered.kill("SIGTERM");
        await closed;
      }
    }
  }
}

if (import.meta.main) {
  const manifest = process.env.DESKTOP_TEST_CANDIDATE;
  const output = process.env.DESKTOP_TEST_SIGNAL_OUTPUT;
  if (!manifest || !output) throw new Error("DESKTOP_TEST_CANDIDATE and DESKTOP_TEST_SIGNAL_OUTPUT are required");
  await verifySignals(manifest, output);
}
