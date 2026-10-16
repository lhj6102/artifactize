// Exercise the public Launcher and actual Tauri service, not a replacement service hook.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { it } from "node:test";
import { environment } from "./environment.ts";
import { executableSuffix, runnerCommand } from "./platform.ts";

const runner = `
import { Launcher } from "@wdio/cli";
import { pathToFileURL } from "node:url";
const launcher = new Launcher(process.argv[1]);
await launcher.initialize();
const configuration = await import(pathToFileURL(process.argv[1]).href);
let outcome;
try { outcome = { returned: await launcher.run() }; }
catch (error) { outcome = { rejected: String(error) }; }
if (process.env.DESKTOP_TEST_BATCH === '["accounts"]') {
  try { await configuration.config.onPrepare(undefined, configuration.config.capabilities); }
  catch (error) { outcome.preparation = { name: error.name, cause: error.cause?.message }; }
}
await configuration.releasePrepared();
process.stdout.write("STARTUP_RESULT " + JSON.stringify(outcome) + "\\n", () => process.exit(0));
`;

type Outcome = { rejected?: string; preparation?: { name: string; cause: string } };

async function launch(root: string, id: string): Promise<Outcome> {
  const fixture = { root: join(root, "owner"), repo: join(root, "owner", "repo"), state: join(root, "owner", "state") };
  const env = {
    ...environment(fixture),
    DESKTOP_TEST_CANDIDATE: join(root, "candidate.json"),
    DESKTOP_TEST_BATCH: JSON.stringify([id]),
    DESKTOP_TEST_BATCH_OUTPUT: join(root, `${id}-logs`),
  };
  const command = runnerCommand(["--input-type=module", "-e", runner, join(import.meta.dirname, "wdio.conf.ts")], env);
  const child = spawn(command.program, command.args, { cwd: resolve(import.meta.dirname, ".."), env: command.env, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk: Buffer) => { stdout += chunk; });
  child.stderr.on("data", (chunk: Buffer) => { stderr += chunk; });
  const code = await new Promise<number | null>((done, fail) => {
    child.once("error", fail);
    child.once("close", done);
  });
  assert.equal(code, 0, stdout + stderr);
  const result = stdout.split("\n").find((line) => line.startsWith("STARTUP_RESULT "));
  assert.ok(result, stdout + stderr);
  return JSON.parse(result.slice("STARTUP_RESULT ".length)) as Outcome;
}

it("a failed preparation stops the real service before its executable boundary", { timeout: 120000 }, async () => {
  const parent = process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir();
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "desktop-startup-test-"));
  const callback = createServer();
  try {
    // A declared invalid executable makes a service launch attempt an immediate,
    // observable error without starting any native app or WebDriver server.
    const app = join(root, `unlaunchable-app${executableSuffix}`);
    writeFileSync(app, "not an executable", { mode: 0o600 });
    writeFileSync(join(root, "candidate.json"), JSON.stringify({ root, cli: app, app, observer: app, manifest: join(root, "candidate.json") }));
    await new Promise<void>((done, fail) => {
      callback.once("error", fail);
      callback.listen(1455, "127.0.0.1", done);
    });
    const blocked = await launch(root, "accounts");
    assert.match(blocked.rejected ?? "", /accounts requires free loopback callback port 1455/);
    assert.doesNotMatch(blocked.rejected ?? "", /Failed to start embedded WebDriver/);
    assert.deepEqual(blocked.preparation, { name: "SevereServiceError", cause: "accounts requires free loopback callback port 1455" });

    // Positive control: a successful setup must reach the same real service's
    // executable boundary, proving the first rejection was before app admission.
    const admitted = await launch(root, "settings-keys");
    assert.match(admitted.rejected ?? "", /Failed to start embedded WebDriver/);
    assert.match(admitted.rejected ?? "", /EACCES|EINVAL|ENOEXEC|spawn/);
  } finally {
    if (callback.listening) await new Promise<void>((done, fail) => callback.close((error) => error ? fail(error) : done()));
    rmSync(root, { recursive: true, force: true });
  }
});
