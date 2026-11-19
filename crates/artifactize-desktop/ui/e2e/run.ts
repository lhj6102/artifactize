// Build once, then let the public WDIO Launcher own each bounded batch of capabilities.
import { inspect } from "node:util";
import { assertAdmissionsStopped, assertAppsStopped, retireOwnAdmissions } from "./observation.ts";
import { checkSuite } from "../lint/e2e-integrity.ts";
import { cleanup, prepare, readCandidate, snapshotCandidate } from "./build.ts";
import {
  absolutePath, appsAtOnce, asNativePath, assertHarnessPlatform, assertRunnerMachine, exists, inheritedEnvironment, listFiles, nativePath, newDirectory,
  nativeCrashReports, nodeProgram, processId, processRunning, readText, requiredVariable, removeTree, setVariable, writeTextOnce, runForwardingInterrupts, runnerCommand, scratchDirectory, serviceFailureLogs, suitePath, variable, type Environment, type NativePath,
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
  assertHarnessPlatform();
  const candidate = readCandidate();
  const runner = nativePath(candidate.root, "running.json");
  if (!writeTextOnce(runner, String(processId()))) throw new Error("candidate already has a batch owner; use a fresh candidate");
  try {
    if (exists(nativePath(candidate.root, "cleaning.json"))) throw new Error("candidate cleanup already started");
    assertAdmissionsStopped(candidate.root, candidate.observer);
    await launchBatch();
  } finally { removeTree(runner); }
}

async function launchBatch(): Promise<void> {
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
    const { selectedScenarios } = await import("./catalog.ts");
    const ids = JSON.parse(requiredVariable("DESKTOP_TEST_BATCH", "each batch owns its app identities")) as string[];
    const cases = selectedScenarios(ids);
    if (code === 0 && cases.some((item) => item.relaunched)) {
      if (cases.length !== 1) throw new Error("a relaunch case owns its own Launcher batch");
      if (!configuration.ownerFiles) throw new Error("the first launch has no app admission journal");
      assertAppsStopped(readCandidate().observer, configuration.ownerFiles);
      setVariable("DESKTOP_TEST_RELAUNCH", "1");
      const next = new Launcher(suitePath("wdio.conf.ts"));
      await next.initialize();
      const again = await next.run();
      if (again !== 0) failures.push(new Error(`relaunched WDIO case failed with exit code ${again}`));
    }
  } catch (error) {
    failures.push(configuration.preparationFailure === undefined ? error
      : new Error("Desktop scenario preparation failed", { cause: configuration.preparationFailure }));
  }
  try {
    await configuration.releasePrepared();
    const candidate = readCandidate();
    retireOwnAdmissions(candidate.root, candidate.observer);
  } catch (error) { failures.push(error); }
  if (failures.length) {
    try {
      const output = asNativePath(requiredVariable("DESKTOP_TEST_BATCH_OUTPUT", "failed batches retain service diagnostics"));
      for (const log of serviceFailureLogs(output)) console.error(log);
    } catch (error) { failures.push(error); }
    throw new AggregateError(failures, "desktop WDIO batch failed");
  }
}

/** The batch runner that owned the candidate has exited. */
function assertBatchEnded(root: NativePath): void {
  const runner = nativePath(root, "running.json");
  if (!exists(runner)) return;
  const pid = Number(readText(runner));
  if (!Number.isSafeInteger(pid) || pid <= 0 || processRunning(pid)) {
    throw new Error("candidate is still owned by its batch runner");
  }
}

async function suite(ids: string[]): Promise<void> {
  const { selectedScenarios } = await import("./catalog.ts");
  const selected = selectedScenarios(ids);
  // A relaunch is one complete scenario with two service-owned app lifetimes. Keep it
  // alone; unrelated cases neither inherit its state nor get run a second time.
  const batches: typeof selected[] = [];
  for (const item of selected) {
    const previous = batches.at(-1);
    if (item.relaunched || !previous || previous.some((item) => item.relaunched) || previous.length === CAPABILITIES_PER_INVOCATION) batches.push([item]);
    else previous.push(item);
  }
  const scratch = scratchDirectory();
  const declaredOutput = variable("ARTIFACTIZE_OUTPUT_DIR");
  const output = newDirectory(declaredOutput === undefined ? scratch : asNativePath(declaredOutput), "desktop-e2e-");
  // The full suite always builds its own evaluated inputs, even when an Agent-tool
  // session has a prepared candidate in the environment. Explicit targeted developer
  // runs may reuse immutable binaries, but still own a private snapshot and fixtures.
  const candidate = await (ids.length > 0 && variable("DESKTOP_TEST_CANDIDATE") ? snapshotCandidate(scratch) : prepare())
    .catch((error: unknown) => {
      if (declaredOutput === undefined) removeTree(output);
      throw error;
    });
  console.error(`Desktop E2E diagnostics: ${output}`);
  const failures: unknown[] = [];
  let confirmed = true;
  try {
    for (const [index, batch] of batches.entries()) {
      const ids = batch.map((item) => item.id);
      const env = {
        ...inheritedEnvironment(),
        DESKTOP_TEST_CANDIDATE: candidate.manifest,
        DESKTOP_TEST_RESOURCES: candidate.root,
        DESKTOP_TEST_BATCH: JSON.stringify(ids),
        DESKTOP_TEST_BATCH_OUTPUT: nativePath(output, `batch-${index + 1}`),
      };
      confirmed = false;
      try {
        const wrapped = runnerCommand([file, "batch"], env);
        const code = await execute(wrapped.program, wrapped.args, wrapped.env);
        if (code !== 0) failures.push(new Error(`scenarios ${ids.join(", ")} failed with exit code ${code}`));
      } catch (error) { failures.push(error); }
      try {
        // The batch runner itself has ended too, not only the apps it admitted: one stopped by
        // a signal during fixture setup may still be preparing inputs.
        assertBatchEnded(candidate.root);
        assertAdmissionsStopped(candidate.root, candidate.observer);
        confirmed = true;
      } catch (error) { failures.push(error); }
      if (!confirmed) break;
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
    if (confirmed) {
      try { await cleanup(candidate.root); } catch (error) { failures.push(error); }
    } else failures.push(new Error(`Native shutdown unconfirmed; retained inputs at ${candidate.root}`));
    if (declaredOutput === undefined) {
      try { removeTree(output); } catch (error) { failures.push(error); }
    }
  }
  if (failures.length) throw new AggregateError(failures, "desktop E2E failures");
}

async function unit(): Promise<void> {
  const harness = absolutePath(suitePath(), "../../harness");
  const files = ["input.test.ts", "commands.test.ts", "keys.test.ts", "observation.test.ts", "runner.test.ts"].map((name) => suitePath(name));
  if (exists(harness)) files.push(...listFiles(harness).filter((path) => path.endsWith(".test.mts")));
  process.exitCode = await execute(nodeProgram, ["--test", ...files], inheritedEnvironment());
}

const mode = process.argv[2] ?? "suite";
try {
  if (mode === "suite" || mode === "prepare") checkSuite();
  if (mode === "suite" || mode === "batch") assertRunnerMachine();
  if (mode === "prepare") console.log((await prepare()).manifest);
  else if (mode === "cleanup") {
    const candidate = readCandidate(process.argv[3]);
    const marker = nativePath(candidate.root, "cleaning.json");
    if (!writeTextOnce(marker, "cleanup\n")) throw new Error("candidate cleanup already started");
    try {
      assertBatchEnded(candidate.root);
      assertAdmissionsStopped(candidate.root, candidate.observer);
      await cleanup(candidate.root);
    } finally { removeTree(marker); }
  } else if (mode === "unit") await unit();
  else if (mode === "batch") await batch();
  else if (mode === "suite") await suite(process.argv.slice(3));
  else throw new Error(`unknown desktop test runner mode: ${mode}`);
} catch (error) {
  console.error(inspect(error, { depth: null, colors: false }));
  process.exitCode = 1;
}
// WDIO's async-exit-hook registers beforeExit with a hard-coded zero. Just like
// WDIO's CLI, exit explicitly after its service and our fixtures have finished.
if (mode === "batch") process.exit(process.exitCode ?? 0);
