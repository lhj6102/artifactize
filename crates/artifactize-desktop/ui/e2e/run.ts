// The suite's runner. `suite [ids...]` builds its own test candidate and runs
// the scenarios in batches of as many apps as this OS
// runs at once, each batch a runner of its own on a headless display, and removes what it
// built. `prepare` and `cleanup <manifest>` build and remove a candidate for later runs, and
// `unit` runs the suite's own unit tests.
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { basename } from "node:path";
import { inspect } from "node:util";
import { BatchLifecycle } from "./batch-lifecycle.ts";
import { checkSuite } from "../lint/e2e-integrity.ts";
import { cleanup, prepare, readCandidate } from "./build.ts";
import { selectedScenarios } from "./catalog.ts";
import { appsAtOnce, assertHeadless, desktopResources, foregroundQuarantine, nodeProgram, packageCommand, runnerCommand, systemName, setVariable, startBatch, variable } from "./platform.ts";
import { asNativePath, exists, makeDirectory, nativePath, newDirectory, readText, removeTree, scratchDirectory, sourceTree, suitePath, withMachineLock, writeText, type NativePath } from "./portable.ts";
import { APPS_UNRELEASED, type Scenario } from "./scenario.ts";

/** Scenarios in batches of at most `size`, in their order; one that runs alone, or whose app
 * is started again (the service restarts it for the batch's second spec), has a batch of its
 * own. */
export function batches(scenarios: readonly Pick<Scenario, "id" | "alone" | "restarted">[], size: number): string[][] {
  const single = (item: Pick<Scenario, "alone" | "restarted"> | undefined) => !!item?.alone || !!item?.restarted;
  const result: string[][] = [];
  for (const item of scenarios) {
    const last = result.at(-1);
    if (single(item) || !last || last.length >= size || last.some((id) => single(scenarios.find((other) => other.id === id)))) result.push([item.id]);
    else last.push(item.id);
  }
  return result;
}

let interrupted = false;

/** Run `program` with this terminal's output until it exits; its exit code. */
function exitCode(started: ReturnType<typeof spawn>): Promise<number> {
  return new Promise((done, fail) => {
    started.once("error", fail);
    started.once("close", (code) => done(code ?? 1));
  });
}

function child(program: string, args: string[], env: NodeJS.ProcessEnv): Promise<number> {
  return exitCode(spawn(program, args, { env, stdio: "inherit" }));
}

/** An interrupt fails the run, whenever it comes, and starts no other batch. One that reaches
 * the running batch too, as a terminal's Ctrl+C does, stops its scenarios: each keeps its
 * evidence, and the batch closes its apps and fixtures (launch). One that reaches this process
 * alone stops no scenario: the running batch runs to its end and is judged as any other, and
 * the run then fails naming that batch, whose logs it keeps, and the scenarios that did not
 * run. */
function interrupt(): void {
  interrupted = true;
  process.exitCode = 1;
}

/** Let the event loop turn once through its timers and its input, where a signal that came
 * while this thread was busy reaches its handler. */
function turned(): Promise<void> {
  return new Promise((done) => setTimeout(() => setImmediate(done), 0));
}

/** Resolves once what this process has printed is written out: a pipe takes output in its own
 * time, and `process.exit` drops what still waits. */
async function printed(): Promise<void> {
  for (const stream of [process.stdout, process.stderr]) await new Promise((done) => stream.write("", done));
}

async function suite(ids: string[]): Promise<void> {
  checkSuite();
  const { selectedScenarios } = await import("./catalog.ts");
  const scenarios = selectedScenarios(ids);
  if (scenarios.some(({ id }) => id === "viewer-builtins-plugin")) {
    // WebDriver checks the outer sandbox in the app; a DOM test checks Point inside it.
    const command = packageCommand(["-C", suitePath(".."), "test", "src/viewers/HtmlExample.test.ts"]);
    if (await child(command.program, command.args, process.env) !== 0) throw new Error("the HTML viewer example test failed");
  }
  const groups = batches(scenarios, appsAtOnce);
  const declared = variable("ARTIFACTIZE_OUTPUT_DIR");
  // Logs of this run's batches, in a folder of their own also in a declared output.
  const output = newDirectory(declared ? asNativePath(declared) : scratchDirectory(), "desktop-e2e-");
  // A failed run's evidence: one folder per run, in DESKTOP_TEST_EVIDENCE (CI uploads it), else
  // in the run's declared output or the scratch folder. It stays only when the run failed.
  const evidence = newDirectory(makeDirectory(asNativePath(variable("DESKTOP_TEST_EVIDENCE") ?? (declared ?? scratchDirectory()))), "desktop-e2e-evidence-");
  // CI runs the whole suite; elsewhere the run stops at the first failing batch.
  const whole = variable("CI") === "true";
  const failed: string[] = [];
  const failures: unknown[] = [];
  /** Whether the failures say already that the run was interrupted. */
  const noted = () => failures.some((failure) => failure instanceof Error && failure.message.startsWith("interrupted"));
  let candidate: Awaited<ReturnType<typeof prepare>> | undefined;
  try {
    candidate = await prepare();
    for (const [index, ids] of groups.entries()) {
      if (interrupted) {
        const left = groups.slice(index).flat();
        failures.push(new Error(`interrupted before the batch ${ids.join(", ")} started: ${left.length} scenarios did not run (${left.join(", ")})`));
        break;
      }
      // The isolated display's framebuffer is written relative to this batch's own cwd.
      const logs = makeDirectory(nativePath(output, `batch-${index + 1}`));
      const env = { ...process.env, DESKTOP_TEST_CANDIDATE: candidate.manifest, DESKTOP_TEST_BATCH: JSON.stringify(ids),
        DESKTOP_TEST_BATCH_OUTPUT: logs, DESKTOP_TEST_EVIDENCE_RUN: evidence };
      const command = runnerCommand([suitePath("run.ts"), "batch"], env);
      const passed = await exitCode(startBatch(command.program, command.args, command.env, logs)) === 0;
      const left = groups.slice(index + 1).flat();
      const unrun = left.length ? `${left.length} scenarios did not run (${left.join(", ")})` : "no scenario was left to run";
      if (!passed) {
        // A batch that ended before it kept its logs, as an interrupt may end it, has them
        // printed and kept here.
        if (!exists(nativePath(evidence, basename(logs)))) for (const log of serviceLogs(logs, evidence)) console.error(log);
        // The scenarios that kept evidence failed; a batch that failed before any did fails whole.
        const kept = ids.filter((id) => exists(nativePath(evidence, id)));
        if (interrupted) {
          failed.push(...kept);
          failures.push(new Error(`interrupted while the batch ${ids.join(", ")} ran, which did not pass: each scenario the interrupt stopped keeps the wait it was in as its evidence; ${unrun}`));
          break;
        }
        failed.push(...(kept.length ? kept : ids));
        if (exists(nativePath(logs, APPS_UNRELEASED))) {
          failures.push(new Error(`stopped because the batch ${ids.join(", ")} may still have live apps; ${unrun}`));
          break;
        }
        if (!whole && left.length) {
          failures.push(new Error(`stopped at the first failing batch; ${unrun} (CI runs them all)`));
          break;
        }
      } else if (interrupted) {
        // The interrupt reached this process alone and stopped no scenario: the batch ran to
        // its end and passed. Its logs are kept as the interrupted run's evidence.
        serviceLogs(logs, evidence);
        failures.push(new Error(`interrupted while the batch ${ids.join(", ")} ran, which ran to its end and passed (its logs are kept): ${unrun}`));
        break;
      }
    }
    if (interrupted && !noted()) failures.push(new Error("interrupted once every batch had run"));
  } catch (error) {
    failures.push(error);
  } finally {
    try {
      if (candidate) cleanup(candidate.root);
    } catch (error) { failures.push(error); }
  }
  if (failed.length) failures.unshift(new Error(`desktop scenarios failed on ${systemName}: ${failed.join(", ")}`));
  // A signal queued while synchronous candidate cleanup held the event loop must be seen
  // before deciding that its logs and evidence may go.
  await turned();
  if (interrupted) {
    if (!noted()) failures.push(new Error("interrupted: the suite was stopped while it cleaned up"));
  }
  // Keep every batch's logs before either output directory can be removed. On success the
  // staged evidence is removed; cleanup failures (not only test failures) retain it.
  try {
    for (const entry of readdirSync(output, { withFileTypes: true })) {
      if (!entry.isDirectory() || exists(nativePath(evidence, entry.name))) continue;
      const logs = serviceLogs(nativePath(output, entry.name), evidence);
      if (failures.length) for (const log of logs) console.error(log);
    }
    if (!declared) removeTree(output);
  } catch (error) { failures.push(error); }
  const kept = failures.length > 0;
  if (!kept) try { removeTree(evidence); } catch (error) { failures.push(error); }
  if (!failures.length) return;
  throw new AggregateError(failures, `desktop E2E failed on ${systemName}${kept ? `; evidence (each failed scenario's failure, screenshot and logs): ${evidence}` : ""}`);
}

/** The tail of each log the service wrote for a failed batch into `directory`, the app's own
 * output among them, to print; each is kept in the run's evidence folder `evidence`. */
function serviceLogs(directory: NativePath, evidence: NativePath | undefined): string[] {
  if (!exists(directory)) return [];
  return sourceTree(directory).filter((item) => item.kind === "file" && item.name.endsWith(".log")).map((item) => {
    const tail = readText(item.path).slice(-65536);
    if (evidence) writeText(nativePath(makeDirectory(nativePath(evidence, basename(directory))), item.name), tail);
    return `--- service log ${item.name} ---\n${tail.slice(-16384)}`;
  });
}

/** Run `body` holding the machine-wide resources the batch's apps use: this OS's desktop
 * ones, and for a scenario that runs alone, its own (E2E-ISOLATED), so that another suite run
 * of this machine waits for them. */
async function holding(body: () => Promise<void>): Promise<void> {
  const ids = JSON.parse(variable("DESKTOP_TEST_BATCH") ?? "[]") as string[];
  const names = [...desktopResources, ...selectedScenarios(ids).filter((item) => item.alone).map((item) => `alone-${item.id}`)];
  const run = (at: number): Promise<void> => at === names.length ? body() : withMachineLock(names[at], () => run(at + 1));
  await run(0);
}

async function batch(): Promise<void> {
  assertHeadless();
  await holding(() => launch(new BatchLifecycle(foregroundQuarantine(),
    `Batch ${variable("DESKTOP_TEST_BATCH") ?? ""} owns the foreground; its output is at ${variable("DESKTOP_TEST_BATCH_OUTPUT")}.`)));
}

async function launch(owner: BatchLifecycle): Promise<void> {
  const output = asNativePath(variable("DESKTOP_TEST_BATCH_OUTPUT") ?? "");
  const declared = variable("DESKTOP_TEST_EVIDENCE_RUN");
  const evidence = declared ? asNativePath(declared) : newDirectory(output, "evidence-");
  setVariable("DESKTOP_TEST_EVIDENCE_RUN", evidence);
  const logs = () => serviceLogs(output, evidence);
  const { Launcher } = await import("@wdio/cli");
  const launcher = new Launcher(suitePath("wdio.conf.ts"));
  let loaded: typeof import("./wdio.conf.ts") | undefined;
  let exiting = false;
  const teardown = async () => {
    if (loaded) await loaded.teardown(owner);
    else await owner.close();
  };
  // An interrupt goes through WDIO's own exit routine, which ends its workers (each keeps the
  // evidence of the scenario it was in, journey.spec.ts); this hook, in the same routine, then
  // stops the apps as the service does at the end, closes the fixtures and prints and keeps the
  // logs as for any failed batch, before the process exits.
  const exitHook = createRequire(import.meta.resolve("@wdio/cli"))("async-exit-hook") as (hook: (done: () => void) => void) => void;
  exitHook((done) => {
    exiting = true;
    const internals = launcher as unknown as { runner?: { shutdown(): Promise<unknown> } };
    void (async () => {
      try { await internals.runner?.shutdown(); }
      catch (error) { console.error(inspect(error)); }
      try { await teardown(); } catch (error) { console.error(inspect(error)); }
      try { for (const log of logs()) console.error(log); } catch (error) { console.error(inspect(error)); }
      await printed();
      done();
    })();
  });
  const failures: unknown[] = [];
  try {
    // Set up the loader before importing the configuration. Neither step may start apps.
    await launcher.initialize();
    if (exiting) throw new Error("interrupted before the desktop service started");
    const configuration = await import("./wdio.conf.ts");
    if (exiting) throw new Error("interrupted before the desktop service started");
    loaded = configuration;
    configuration.trackServiceShutdown(owner, () => (launcher as unknown as { _launcher?: import("./wdio.conf.ts").Service[] })._launcher ?? []);
    const code = await launcher.run();
    if (code !== 0) failures.push(new Error(`the WDIO batch failed with exit code ${code}`));
    else configuration.assertCompleted();
  } catch (error) {
    failures.push(loaded?.preparationFailure ?? error);
  } finally {
    try { await teardown(); } catch (error) { failures.push(error); }
  }
  if (failures.length) {
    for (const log of logs()) console.error(log);
    throw new AggregateError(failures, "desktop WDIO batch failed");
  }
}

async function unit(): Promise<void> {
  const tests = ["input.test.ts", "keys.test.ts", "runner.test.ts"].map((name) => suitePath(name));
  process.exitCode = await child(nodeProgram, ["--test", ...tests], process.env);
}

// The runner's entry needs import.meta.main (Node 24.2 and later): an older Node fails here
// instead of running nothing.
if (import.meta.main === undefined) throw new Error(`the desktop test runner needs Node 24.2 or later, not ${process.version}`);
if (import.meta.main) {
  const mode = process.argv[2] ?? "suite";
  // Track interruption separately from the child status, including between prepare's steps.
  if (mode === "suite" || mode === "prepare") {
    process.on("SIGINT", interrupt);
    process.on("SIGTERM", interrupt);
  }
  try {
    if (mode === "suite") await suite(process.argv.slice(3));
    else if (mode === "batch") {
      // Until it has passed, a batch that ends, by an interrupt too, has failed.
      process.exitCode = 1;
      await batch();
      process.exitCode = 0;
    }
    else if (mode === "prepare") {
      const candidate = await prepare();
      await turned();
      if (interrupted) {
        cleanup(candidate.root);
        throw new Error("interrupted while preparing the E2E candidate");
      }
      console.log(candidate.manifest);
    }
    else if (mode === "cleanup") cleanup(readCandidate(process.argv[3]).root);
    else if (mode === "unit") await unit();
    else throw new Error(`unknown desktop test runner mode: ${mode}`);
  } catch (error) {
    console.error(inspect(error, { depth: null, colors: false }));
    process.exitCode = 1;
  }
  // WDIO's exit hooks would end the batch with status 0: exit with the batch's own, once its
  // output is written out.
  if (mode === "batch") {
    await printed();
    process.exit(process.exitCode ?? 0);
  }
}
