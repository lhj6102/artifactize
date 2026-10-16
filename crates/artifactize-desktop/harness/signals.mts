// Real registered-start signal seams. Invoke with an explicit prepared candidate and
// DESKTOP_TEST_SIGNAL_OUTPUT under the caller's private scratch; never builds an app.
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { readCandidate } from "../ui/e2e/build.ts";

async function bounded<T>(operation: Promise<T>, label: string): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_done, fail) => {
      timeout = setTimeout(() => fail(new Error(`${label} did not complete`)), 120000);
    })]);
  } finally { clearTimeout(timeout); }
}

export async function verifySignals(manifest: string, output: string): Promise<void> {
  readCandidate(manifest);
  mkdirSync(output, { recursive: true });
  const harness = fileURLToPath(new URL("./harness.mts", import.meta.url));
  for (const phase of ["startup", "ready"] as const) {
    for (const signal of ["SIGINT", "SIGTERM"] as const) {
      const directory = mkdtempSync(join(output, `${phase}-${signal}-`));
      const env = { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, DESKTOP_TEST_CANDIDATE: manifest };
      const registered = spawn(process.execPath, [harness, "start"], { env, stdio: ["ignore", "pipe", "pipe"] });
      const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
        registered.once("error", fail);
        registered.once("close", (code, signal) => done({ code, signal }));
      });
      let acquired!: () => void;
      const boundary = new Promise<void>((done) => { acquired = done; });
      let stdout = "";
      let diagnostics = "";
      const ports = new Set<number>();
      const record = (file: string, text: string) => writeFileSync(join(directory, file), text);
      registered.stdout.on("data", (chunk: Buffer) => {
        stdout += chunk.toString();
        record("stdout.log", stdout);
        if (phase === "ready" && stdout.includes("ready\n")) acquired();
      });
      registered.stderr.on("data", (chunk: Buffer) => {
        diagnostics += chunk.toString();
        record("stderr.log", diagnostics);
        for (const match of diagnostics.matchAll(/WebDriver server ready on port (\d+)/g)) ports.add(Number(match[1]));
        // Upstream service has acquired a real app, but not yet returned its Browser.
        if (phase === "startup" && diagnostics.includes("Tauri app spawned (PID:")) acquired();
      });
      const invoke = (mode: string, args: Record<string, unknown> = {}) => new Promise<string>((done, fail) => {
        const command = execFile(process.execPath, [harness, mode], { env, encoding: "utf8", timeout: 60000 },
          (error, stdout) => error ? fail(error) : done(stdout));
        command.stdin!.end(JSON.stringify({ args }));
      });
      let finished = false;
      try {
        await bounded(Promise.race([boundary, closed.then((status) => {
          throw new Error(`registered start exited before ${phase}: ${JSON.stringify(status)}\n${diagnostics}`);
        })]), `${phase} acquisition`);
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
        const status = await bounded(closed, `${phase} ${signal} cleanup`);
        finished = true;
        assert.deepEqual(status, { code: 0, signal: null }, diagnostics);
        assert.equal(stdout, phase === "startup" ? "" : "ready\n", "cancelled startup must never announce readiness");
        assert.equal(existsSync(join(directory, "control.json")), false);
        assert.deepEqual(readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isDirectory()), [],
          "service and public Run cleanup must finish before candidate/fixture removal");
        assert.ok(ports.size > 0, "the real service actually started its embedded listener");
        for (const port of ports) {
          await assert.rejects(fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(2000) }),
            "no service-owned app listener may remain after registered wrapper exit");
        }
        record("result.json", JSON.stringify({ phase, signal, status, ports: [...ports], inputsRemoved: true }, null, 2));
        console.log(`${phase} ${signal}: GREEN; standard owner exited, app listener closed, durable work and inputs released`);
      } finally {
        if (!finished && registered.exitCode === null && registered.signalCode === null) {
          // Even a failed assertion requests the same public registered-owner shutdown;
          // never kill native apps/drivers/displays or remove their inputs out of order.
          registered.kill("SIGTERM");
          await closed;
        }
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
