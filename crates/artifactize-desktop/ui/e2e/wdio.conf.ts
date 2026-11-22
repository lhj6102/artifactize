// One batch of scenarios: WebdriverIO's Tauri service starts and stops each scenario's app
// (E2E-INPUT); this configuration prepares their fixtures before and the runner closes them
// after (teardown).
import { inspect } from "node:util";
import { SevereServiceError } from "webdriverio";
import { checkE2eApp, readCandidate } from "./build.ts";
import { selectedScenarios } from "./catalog.ts";
import { own, type Owned } from "./fixtures/owned.ts";
import { environment } from "./fixtures/repo.ts";
import { appsAtOnce, displayEnvironment, displayScreenshot, foregroundQuarantine, quarantineForeground, releaseForeground, setVariable, variable, type Environment } from "./platform.ts";
import { asNativePath, embeddedPorts, exists, makeDirectory, nativePath, readBytes, readJson, suitePath, writeJson, writeText } from "./portable.ts";
import { APPS_UNRELEASED, COMPLETED, LAST_WINDOW, keepEvidence, type Context } from "./scenario.ts";

const candidate = readCandidate();
const selected = selectedScenarios(JSON.parse(variable("DESKTOP_TEST_BATCH") ?? "[]") as string[]);
if (!selected.length) throw new Error("a WDIO batch needs its scenarios in DESKTOP_TEST_BATCH");
if (selected.length > appsAtOnce || (selected.length > 1 && selected.some((item) => item.alone))) {
  throw new Error("this WDIO batch exceeds the platform's isolated app capacity");
}
const output = makeDirectory(asNativePath(variable("DESKTOP_TEST_BATCH_OUTPUT") ?? ""));
const evidence = asNativePath(variable("DESKTOP_TEST_EVIDENCE_RUN") ?? nativePath(output, "evidence"));
const owned: Owned[] = [];
// A scenario whose app is started again runs alone, as a second spec after its first: the
// service starts the app again for it, as it does for any spec whose app has gone.
const restarts = selected.some((item) => item.restarted);
if (restarts && selected.length > 1) throw new Error("a scenario that starts its app again runs alone");

/** Why preparing the batch failed, for the runner: WDIO wraps it. */
export type Service = {
  onPrepare?: (...args: never[]) => Promise<void>;
  onComplete?: (code: number, config: unknown, caps: unknown) => Promise<void>;
  // The pinned service correlates its app child handles with the configured embedded ports.
  embeddedConfigs?: Map<string, { port: number }>;
  embeddedProcesses?: Map<string, { proc: { pid?: number; exitCode: number | null; signalCode: string | null } }>;
};
const foreground = foregroundQuarantine();
const scenarioPorts = new Map<string, number>();
let observeService: (() => Service[]) | undefined;
const shutdownFailures: unknown[] = [];
const pendingStops = new Map<Service, () => Promise<void>>();
const workers = new Map<string, { id: string; restarted: boolean }>();

/** WDIO can log an ordinary service error without propagating it to the batch result. */
export function trackServiceShutdown(services: () => Service[]): void { observeService = services; }

export let preparationFailure: unknown;

/** Fail unless every scenario of the batch ran to its end: a Launcher that found no spec, or a
 * worker that stopped before its test, ends without failing (E2E-EXPLICIT-FAILURE). */
export function assertCompleted(): void {
  const missing = selected.filter((_item, index) => !owned[index] || !exists(nativePath(owned[index].root, COMPLETED)));
  if (missing.length) throw new Error(`no completed run for ${missing.map((item) => item.id).join(", ")}`);
}

/** Close what each scenario started and delete its files; called once the service has
 * stopped every app of the batch. A scenario whose teardown fails is the one that failed: the
 * failure names it and is kept as its evidence, with the window as its flow left it. When the
 * service failed to stop the batch's apps (`serviceFailure`), every scenario of the batch keeps
 * that failure as its evidence, with the window as its flow left it, before its files go. */
let tearingDown: Promise<void> | undefined;
export function teardown(): Promise<void> {
  return tearingDown ??= finishTeardown();
}

async function finishTeardown(): Promise<void> {
  if (pendingStops.size) {
    for (const [service, stop] of pendingStops) {
      // Retry the service, not a PID found on the machine. It still owns an unsuccessful stop.
      try { await stop(); pendingStops.delete(service); } catch (error) { shutdownFailures.push(error); }
    }
  }
  if (pendingStops.size) {
    const failure = `Tauri service did not confirm that the apps in ${selected.map((item) => item.id).join(", ")} exited. Candidate and inputs kept at ${candidate.root}.`;
    quarantineForeground(foreground, failure);
    writeText(nativePath(output, APPS_UNRELEASED), `${failure}\n`);
  } else {
    releaseForeground(foreground);
  }
  const serviceFailure = shutdownFailures.length ? new AggregateError(shutdownFailures, "Tauri service failed to stop its apps") : undefined;
  const scopes = owned.splice(0);
  // Read before the folders go.
  const windows = scopes.map((scope) => nativePath(scope.root, LAST_WINDOW)).map((image) => (exists(image) ? readBytes(image) : undefined));
  const failures: unknown[] = serviceFailure ? [serviceFailure] : [];
  if (serviceFailure !== undefined) {
    for (const [index] of scopes.entries()) {
      const failure = new Error(`the Tauri service failed to stop the apps of the batch (${selected.map((each) => each.id).join(", ")}) once ${selected[index].id}'s flow had ended`, { cause: serviceFailure });
      await keepEvidence(selected[index].id, nativePath(evidence, selected[index].id), inspect(failure, { depth: null }), windows[index]).catch((cause: unknown) => failures.push(cause));
    }
  }
  // The filesystem may allow removing files while a live app still uses them.
  if (pendingStops.size) throw new AggregateError(failures, "apps not released; scenario inputs kept");
  const results = await Promise.allSettled(scopes.map((scope) => scope.close()));
  for (const [index, result] of results.entries()) {
    if (result.status !== "rejected") continue;
    const failure = new Error(`the teardown of ${selected[index].id} failed, once its flow had ended and its app had stopped: no wait of it was running`, { cause: result.reason });
    failures.push(failure);
    await keepEvidence(selected[index].id, nativePath(evidence, selected[index].id), inspect(failure, { depth: null }), windows[index]).catch((cause: unknown) => failures.push(cause));
  }
  if (failures.length) throw new AggregateError(failures, "scenario teardown failed");
}

type Capability = {
  browserName: string;
  "artifactize:scenario": string;
  "artifactize:context"?: string;
  "wdio:enforceWebDriverClassic": boolean;
  "wdio:tauriServiceOptions"?: Record<string, unknown>;
  "tauri:options"?: { application: string };
};

/** The variables of `env` that are set. */
const set = (env: Environment) => Object.fromEntries(Object.entries(env).filter((entry): entry is [string, string] => entry[1] !== undefined));

export const config = {
  runner: "local",
  specs: restarts ? [suitePath("journey.spec.ts"), suitePath("restarted.spec.ts")] : [suitePath("journey.spec.ts")],
  framework: "mocha",
  reporters: ["spec"],
  logLevel: "warn",
  outputDir: output,
  maxInstances: selected.length,
  maxInstancesPerCapability: 1,
  autoXvfb: false,
  bail: 0,
  specFileRetries: 0,
  connectionRetryCount: 0,
  waitforTimeout: 15000,
  capabilities: selected.map((item): Capability => ({
    browserName: "tauri", "artifactize:scenario": item.id, "wdio:enforceWebDriverClassic": true,
  })),
  services: [["tauri", { appBinaryPath: candidate.app, driverProvider: "embedded", startTimeout: 60000, logDir: output, captureBackendLogs: true }]],
  async onPrepare(_config: unknown, capabilities: Capability[]) {
    try {
      for (const service of observeService?.() ?? []) {
        const complete = service.onComplete?.bind(service);
        if (!complete) continue;
        const prepare = service.onPrepare?.bind(service);
        if (prepare) service.onPrepare = async (...args) => {
          // A failed service startup may leave an app but skip the usual completion hook.
          pendingStops.set(service, () => complete(1, {}, []));
          await prepare(...args);
        };
        service.onComplete = async (...args) => {
          const stop = () => complete(...args);
          pendingStops.set(service, stop);
          try { await stop(); pendingStops.delete(service); }
          catch (error) { shutdownFailures.push(error); throw error; }
        };
      }
      const unset = new Set<string>();
      const prepared: { scope: Owned; input: Awaited<ReturnType<typeof selected[number]["prepare"]>>; env: Environment; app: Context["root"] }[] = [];
      for (const item of selected) {
        const scope = own(nativePath(candidate.root, "fixtures"));
        owned.push(scope);
        const entry = await (async () => {
          const input = await item.prepare(scope);
          const env = { ...environment(input.fixture), ...displayEnvironment(), ARTIFACTIZE_NO_UPDATE_CHECK: "1", ...input.env };
          const app = asNativePath(input.app ?? candidate.app);
          checkE2eApp(app, scope.root, env);
          return { scope, input, env, app };
        })().catch(async (cause: unknown) => {
          // The scenario whose preparation failed is the one that failed: the failure names it
          // and is kept as its evidence. No scenario of its batch ran.
          const failure = new Error(`preparing ${item.id} failed, before any app of its batch (${selected.map((each) => each.id).join(", ")}) started: ${cause instanceof Error ? cause.message : String(cause)}`, { cause });
          await keepEvidence(item.id, nativePath(evidence, item.id), inspect(failure, { depth: null }), undefined);
          throw failure;
        });
        for (const [name, value] of Object.entries(entry.env)) if (value === undefined) unset.add(name);
        prepared.push(entry);
      }
      // Ports of this batch's own (their claims last as long as this process), taken once
      // every fixture is ready.
      const { ports, close } = await embeddedPorts(selected.length);
      prepared[0]?.scope.defer(close);
      for (const [index, { scope, input, env, app }] of prepared.entries()) {
        scenarioPorts.set(selected[index].id, ports[index]);
        const context = nativePath(scope.root, "context.json");
        writeJson(context, { ...input, root: scope.root, port: ports[index], evidence: nativePath(evidence, selected[index].id) } satisfies Context);
        capabilities[index]["artifactize:context"] = context;
        capabilities[index]["tauri:options"] = { application: app };
        capabilities[index]["wdio:tauriServiceOptions"] = {
          appArgs: ["--state-dir", input.fixture.state, "--reviewer", "alice", ...(input.args ?? [])], env: set(env), embeddedPort: ports[index],
        };
      }
      // The service starts each app with this process's variables under the scenario's own:
      // none of the runner's reaches an app unless its scenario sets it, and none a scenario
      // removes comes back from here.
      const safe: Environment = { ...environment({ root: makeDirectory(nativePath(candidate.root, "runner")) }), ...displayEnvironment() };
      for (const name of unset) delete safe[name];
      for (const name of Object.keys(process.env)) setVariable(name, undefined);
      for (const [name, value] of Object.entries(safe)) setVariable(name, value);
    } catch (cause) {
      preparationFailure = cause;
      // Only a severe service error stops WDIO before it starts an app.
      const failure = new SevereServiceError(`Desktop scenario preparation failed: ${cause instanceof Error ? cause.message : String(cause)}`);
      failure.cause = cause;
      throw failure;
    }
  },
  onWorkerStart(cid: string, capability: Capability, specs: string[]) {
    workers.set(cid, { id: capability["artifactize:scenario"], restarted: specs.some((spec) => spec.endsWith("restarted.spec.ts")) });
  },
  async onWorkerEnd(cid: string, code: number) {
    const worker = workers.get(cid);
    workers.delete(cid);
    if (code === 0 || !worker?.id) return;
    const folder = nativePath(evidence, worker.restarted ? `${worker.id}-started-again` : worker.id);
    if (exists(nativePath(folder, "failure.txt"))) return; // afterTest had a live session.
    let screen: Uint8Array | undefined;
    let capture = "";
    const port = scenarioPorts.get(worker.id);
    const process = (observeService?.() ?? []).flatMap((service) => [...(service.embeddedConfigs ?? [])]
      .filter(([, config]) => config.port === port)
      .map(([key]) => service.embeddedProcesses?.get(key)?.proc))
      .find((child) => child?.exitCode === null && child.signalCode === null);
    try { screen = displayScreenshot(output, process?.pid); }
    catch (error) { capture = `\nCould not capture the owned window: ${String(error)}`; }
    await keepEvidence(worker.id, folder, `WebdriverIO worker ${cid} exited ${code} before its test could keep evidence; see its session/setup error in the batch log.${capture}`, screen);
  },
  beforeSession(_config: unknown, capability: Capability) {
    setVariable("DESKTOP_TEST_SCENARIO", capability["artifactize:scenario"]);
    setVariable("DESKTOP_TEST_CONTEXT", capability["artifactize:context"]);
  },
  /** A failed test's evidence (E2E-FAILURE-EVIDENCE), a timeout's too, taken while its app
   * still runs. */
  async afterTest(_test: unknown, _context: unknown, result: { passed: boolean; error?: Error }) {
    if (result.passed) return;
    const context = readJson(asNativePath(variable("DESKTOP_TEST_CONTEXT") ?? "")) as Context;
    const failure = result.error ? result.error.stack ?? result.error.message : "the test failed without an error";
    await keepEvidence(variable("DESKTOP_TEST_SCENARIO") ?? "", context.evidence, failure, (globalThis as { browser?: WebdriverIO.Browser }).browser);
  },
  mochaOpts: { timeout: Math.max(...selected.map((item) => item.timeout ?? 180000)) },
};
