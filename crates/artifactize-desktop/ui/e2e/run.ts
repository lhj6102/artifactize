// Build once, then let the public WDIO Launcher own each bounded batch of capabilities.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { inspect } from "node:util";
import { cleanup, prepare, readCandidate, snapshotCandidate } from "./build.ts";
import { assertRunnerMachine, runnerCommand } from "./platform.ts";

const file = join(import.meta.dirname, "run.ts");
// Embedded service launches every capability during onPrepare. Bounding the number
// supplied to one Launcher, rather than scheduling app lifetimes ourselves, bounds
// simultaneous WebViews and workers on ordinary CI machines.
const CAPABILITIES_PER_INVOCATION = 2;
let interrupted = false;

async function execute(program: string, args: string[], env: NodeJS.ProcessEnv): Promise<number> {
  const child = spawn(program, args, { env, cwd: resolve(import.meta.dirname, ".."), stdio: "inherit" });
  const interrupt = () => { interrupted = true; child.kill("SIGINT"); };
  const terminate = () => { interrupted = true; child.kill("SIGTERM"); };
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", terminate);
  try {
    let failure: Error | undefined;
    child.once("error", (error) => { failure = error; });
    const { code, signal } = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done) => {
      child.once("close", (code, signal) => done({ code, signal }));
    });
    if (failure) throw failure;
    if (signal) throw new Error(`runner terminated by ${signal}`);
    return code ?? 1;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", terminate);
  }
}

async function batch(): Promise<void> {
  const { Launcher } = await import("@wdio/cli");
  const launcher = new Launcher(join(import.meta.dirname, "wdio.conf.ts"));
  // Initialize the public loader before importing its config, so the fixture owner
  // and WDIO use the same module instance and callbacks never leave this process.
  await launcher.initialize();
  const configuration = await import("./wdio.conf.ts");
  const failures: unknown[] = [];
  try {
    const code = await launcher.run();
    if (code !== 0) failures.push(new Error(`WDIO batch failed with exit code ${code}`));
  } catch (error) {
    failures.push(configuration.preparationFailure === undefined ? error
      : new Error("Desktop scenario preparation failed", { cause: configuration.preparationFailure }));
  }
  try { await configuration.releasePrepared(); } catch (error) { failures.push(error); }
  if (failures.length) throw new AggregateError(failures, "desktop WDIO batch failed");
}

async function suite(ids: string[]): Promise<void> {
  const { selectedScenarios } = await import("./catalog.ts");
  const selected = selectedScenarios(ids);
  const scratch = process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir();
  const outputParent = process.env.ARTIFACTIZE_OUTPUT_DIR ?? scratch;
  mkdirSync(outputParent, { recursive: true });
  const output = mkdtempSync(join(outputParent, "desktop-e2e-"));
  // The full suite always builds its own evaluated inputs, even when an Agent-tool
  // session has a prepared candidate in the environment. Explicit targeted developer
  // runs may reuse immutable binaries, but still own a private snapshot and fixtures.
  const candidate = ids.length > 0 && process.env.DESKTOP_TEST_CANDIDATE ? await snapshotCandidate(scratch) : await prepare();
  console.error(`Desktop E2E diagnostics: ${output}`);
  const failures: unknown[] = [];
  try {
    for (let offset = 0; offset < selected.length; offset += CAPABILITIES_PER_INVOCATION) {
      const ids = selected.slice(offset, offset + CAPABILITIES_PER_INVOCATION).map((item) => item.id);
      const env = {
        ...process.env,
        DESKTOP_TEST_CANDIDATE: candidate.manifest,
        DESKTOP_TEST_RESOURCES: candidate.root,
        DESKTOP_TEST_BATCH: JSON.stringify(ids),
        DESKTOP_TEST_BATCH_OUTPUT: join(output, `batch-${offset / CAPABILITIES_PER_INVOCATION + 1}`),
      };
      try {
        const code = await execute(process.execPath, [file, "batch"], env);
        if (code !== 0) failures.push(new Error(`scenarios ${ids.join(", ")} failed with exit code ${code}`));
      } catch (error) { failures.push(error); }
      if (interrupted) {
        failures.push(new Error("desktop E2E was interrupted; no further batches were started"));
        break;
      }
    }
  } finally {
    try { await cleanup(candidate.root); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "desktop E2E failures");
}

async function unit(): Promise<void> {
  const harness = resolve(import.meta.dirname, "../../harness");
  const files = ["commands.test.ts", "keys.test.ts", "observation.test.ts", "runner.test.ts"].map((name) => join(import.meta.dirname, name));
  if (existsSync(harness)) files.push(...readdirSync(harness).filter((name) => name.endsWith(".test.mts")).sort().map((name) => join(harness, name)));
  process.exitCode = await execute(process.execPath, ["--test", ...files], process.env);
}

const mode = process.argv[2] ?? "suite";
try {
  if (mode === "suite" || mode === "batch") assertRunnerMachine();
  if (mode === "prepare") console.log((await prepare()).manifest);
  else if (mode === "cleanup") await cleanup(readCandidate(process.argv[3]).root);
  else if (mode === "unit") await unit();
  else if (mode === "batch") await batch();
  else if (mode === "suite") {
    if (process.env.DESKTOP_TEST_WRAPPED === "1") await suite(process.argv.slice(3));
    else {
      const wrapped = runnerCommand([file, "suite", ...process.argv.slice(3)]);
      process.exitCode = await execute(wrapped.program, wrapped.args, wrapped.env);
    }
  } else throw new Error(`unknown desktop test runner mode: ${mode}`);
} catch (error) {
  console.error(inspect(error, { depth: null, colors: false }));
  process.exitCode = 1;
}
// WDIO's async-exit-hook registers beforeExit with a hard-coded zero. Just like
// WDIO's CLI, exit explicitly after its service and our fixtures have finished.
if (mode === "batch") process.exit(process.exitCode ?? 0);
