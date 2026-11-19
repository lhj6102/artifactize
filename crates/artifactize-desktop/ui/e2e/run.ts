// The suite's runner. `suite [ids...]` builds a candidate (or, for named scenarios, copies the
// one DESKTOP_TEST_CANDIDATE names), runs the scenarios in batches of as many apps as this OS
// runs at once, each batch a runner of its own on a headless display, and removes what it
// built. `prepare` and `cleanup <manifest>` build and remove a candidate for later runs, and
// `unit` runs the suite's own unit tests.
import { spawn } from "node:child_process";
import { inspect } from "node:util";
import { checkSuite } from "../lint/e2e-integrity.ts";
import { cleanup, prepare, readCandidate, snapshotCandidate } from "./build.ts";
import { appsAtOnce, assertHeadless, nodeProgram, runnerCommand, variable } from "./platform.ts";
import { asNativePath, exists, nativePath, newDirectory, readText, removeTree, scratchDirectory, sourceTree, suitePath, type NativePath } from "./portable.ts";
import type { Scenario } from "./scenario.ts";

/** Scenarios in batches of at most `size`, in their order; one that runs alone has a batch of
 * its own. */
export function batches(scenarios: readonly Pick<Scenario, "id" | "alone">[], size: number): string[][] {
  const result: string[][] = [];
  for (const item of scenarios) {
    const last = result.at(-1);
    if (item.alone || !last || last.length >= size || last.some((id) => scenarios.find((other) => other.id === id)?.alone)) result.push([item.id]);
    else last.push(item.id);
  }
  return result;
}

let interrupted = false;

/** Run `program` with this terminal's output until it exits; its exit code. */
function child(program: string, args: string[], env: NodeJS.ProcessEnv): Promise<number> {
  return new Promise((done, fail) => {
    const started = spawn(program, args, { env, stdio: "inherit" });
    started.once("error", fail);
    started.once("close", (code) => done(code ?? 1));
  });
}

/** An interrupt (a terminal's Ctrl+C reaches the batch too) lets the running batch close its
 * apps and fixtures, starts no other, and fails the run. */
function interrupt(): void {
  interrupted = true;
}

async function suite(ids: string[]): Promise<void> {
  checkSuite();
  const { selectedScenarios } = await import("./catalog.ts");
  const groups = batches(selectedScenarios(ids), appsAtOnce);
  const declared = variable("ARTIFACTIZE_OUTPUT_DIR");
  const output = declared ? asNativePath(declared) : newDirectory(scratchDirectory(), "desktop-e2e-");
  const failed: string[] = [];
  const failures: unknown[] = [];
  let candidate: Awaited<ReturnType<typeof prepare>> | undefined;
  try {
    candidate = ids.length > 0 && variable("DESKTOP_TEST_CANDIDATE") ? await snapshotCandidate(scratchDirectory()) : await prepare();
    for (const [index, ids] of groups.entries()) {
      if (interrupted) break;
      const env = { ...process.env, DESKTOP_TEST_CANDIDATE: candidate.manifest, DESKTOP_TEST_BATCH: JSON.stringify(ids),
        DESKTOP_TEST_BATCH_OUTPUT: nativePath(output, `batch-${index + 1}`) };
      const command = runnerCommand([suitePath("run.ts"), "batch"], env);
      if (await child(command.program, command.args, command.env) !== 0) failed.push(...ids);
    }
    if (interrupted) failures.push(new Error("interrupted: the suite did not run to its end"));
  } catch (error) {
    failures.push(error);
  } finally {
    try { if (candidate) cleanup(candidate.root); } catch (error) { failures.push(error); }
    if (!declared) try { removeTree(output); } catch (error) { failures.push(error); }
  }
  if (failed.length) failures.unshift(new Error(`desktop scenarios failed: ${failed.join(", ")}`));
  if (failures.length) throw new AggregateError(failures, "desktop E2E failed");
}

/** The tail of each log the service wrote for a failed batch. */
function serviceLogs(directory: NativePath): string[] {
  if (!exists(directory)) return [];
  return sourceTree(directory).filter((item) => item.kind === "file" && item.name.endsWith(".log"))
    .map((item) => `--- service log ${item.name} ---\n${readText(item.path).slice(-16384)}`);
}

async function batch(): Promise<void> {
  assertHeadless();
  const output = asNativePath(variable("DESKTOP_TEST_BATCH_OUTPUT") ?? "");
  const { Launcher } = await import("@wdio/cli");
  const launcher = new Launcher(suitePath("wdio.conf.ts"));
  // The loader is set up before the configuration is imported, so the runner and WDIO share
  // one instance of it.
  await launcher.initialize();
  const configuration = await import("./wdio.conf.ts");
  const failures: unknown[] = [];
  try {
    const code = await launcher.run();
    if (code !== 0) failures.push(new Error(`the WDIO batch failed with exit code ${code}`));
    else configuration.assertCompleted();
  } catch (error) {
    failures.push(configuration.preparationFailure ?? error);
  }
  try { await configuration.teardown(); } catch (error) { failures.push(error); }
  if (failures.length) {
    for (const log of serviceLogs(output)) console.error(log);
    throw new AggregateError(failures, "desktop WDIO batch failed");
  }
}

async function unit(): Promise<void> {
  const tests = ["input.test.ts", "keys.test.ts", "runner.test.ts"].map((name) => suitePath(name));
  process.exitCode = await child(nodeProgram, ["--test", ...tests], process.env);
}

if (import.meta.main) {
  const mode = process.argv[2] ?? "suite";
  // The suite waits for its running batch; a batch, like any other mode, ends on an interrupt.
  if (mode === "suite") {
    process.on("SIGINT", interrupt);
    process.on("SIGTERM", interrupt);
  }
  try {
    if (mode === "suite") await suite(process.argv.slice(3));
    else if (mode === "batch") await batch();
    else if (mode === "prepare") console.log((await prepare()).manifest);
    else if (mode === "cleanup") cleanup(readCandidate(process.argv[3]).root);
    else if (mode === "unit") await unit();
    else throw new Error(`unknown desktop test runner mode: ${mode}`);
  } catch (error) {
    console.error(inspect(error, { depth: null, colors: false }));
    process.exitCode = 1;
  }
  // WDIO's exit hooks would end the batch with status 0: exit with the batch's own.
  if (mode === "batch") process.exit(process.exitCode ?? 0);
}
