// Build once, then let the public WDIO Launcher own each bounded batch of capabilities.
import { inspect } from "node:util";
import { cleanup, prepare, readCandidate, snapshotCandidate } from "./build.ts";
import {
  absolutePath, appsAtOnce, asNativePath, assertRunnerMachine, exists, inheritedEnvironment, listFiles, nativePath, newDirectory,
  nativeCrashReports, nodeProgram, runForwardingInterrupts, runnerCommand, scratchDirectory, suitePath, variable, type Environment,
} from "./platform.ts";

const file = suitePath("run.ts");
const ui = absolutePath(suitePath(), "..");
// Embedded service launches every capability during onPrepare. Bounding the number
// supplied to one Launcher, rather than scheduling app lifetimes ourselves, bounds
// simultaneous WebViews and workers on ordinary CI machines.
const CAPABILITIES_PER_INVOCATION = appsAtOnce;
let interrupted = false;

function execute(program: string, args: string[], env: Environment): Promise<number> {
  return runForwardingInterrupts(program, args, env, ui, () => { interrupted = true; });
}

async function batch(): Promise<void> {
  const { Launcher } = await import("@wdio/cli");
  const launcher = new Launcher(suitePath("wdio.conf.ts"));
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
  const scratch = scratchDirectory();
  const declaredOutput = variable("ARTIFACTIZE_OUTPUT_DIR");
  const output = newDirectory(declaredOutput === undefined ? scratch : asNativePath(declaredOutput), "desktop-e2e-");
  // The full suite always builds its own evaluated inputs, even when an Agent-tool
  // session has a prepared candidate in the environment. Explicit targeted developer
  // runs may reuse immutable binaries, but still own a private snapshot and fixtures.
  const candidate = ids.length > 0 && variable("DESKTOP_TEST_CANDIDATE") ? await snapshotCandidate(scratch) : await prepare();
  console.error(`Desktop E2E diagnostics: ${output}`);
  const failures: unknown[] = [];
  try {
    for (let offset = 0; offset < selected.length; offset += CAPABILITIES_PER_INVOCATION) {
      const ids = selected.slice(offset, offset + CAPABILITIES_PER_INVOCATION).map((item) => item.id);
      const env = {
        ...inheritedEnvironment(),
        DESKTOP_TEST_CANDIDATE: candidate.manifest,
        DESKTOP_TEST_RESOURCES: candidate.root,
        DESKTOP_TEST_BATCH: JSON.stringify(ids),
        DESKTOP_TEST_BATCH_OUTPUT: nativePath(output, `batch-${offset / CAPABILITIES_PER_INVOCATION + 1}`),
      };
      try {
        const code = await execute(nodeProgram, [file, "batch"], env);
        if (code !== 0) failures.push(new Error(`scenarios ${ids.join(", ")} failed with exit code ${code}`));
      } catch (error) { failures.push(error); }
      if (interrupted) {
        failures.push(new Error("desktop E2E was interrupted; no further batches were started"));
        break;
      }
    }
  } finally {
    if (failures.length) {
      try { for (const report of nativeCrashReports(candidate.app)) console.error(report); }
      catch (error) { failures.push(error); }
    }
    try { await cleanup(candidate.root); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "desktop E2E failures");
}

async function unit(): Promise<void> {
  const harness = absolutePath(suitePath(), "../../harness");
  const files = ["commands.test.ts", "keys.test.ts", "observation.test.ts", "runner.test.ts"].map((name) => suitePath(name));
  if (exists(harness)) files.push(...listFiles(harness).filter((path) => path.endsWith(".test.mts")));
  process.exitCode = await execute(nodeProgram, ["--test", ...files], inheritedEnvironment());
}

const mode = process.argv[2] ?? "suite";
try {
  if (mode === "suite" || mode === "batch") assertRunnerMachine();
  if (mode === "prepare") console.log((await prepare()).manifest);
  else if (mode === "cleanup") await cleanup(readCandidate(process.argv[3]).root);
  else if (mode === "unit") await unit();
  else if (mode === "batch") await batch();
  else if (mode === "suite") {
    if (variable("DESKTOP_TEST_WRAPPED") === "1") await suite(process.argv.slice(3));
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
