// One batch of scenarios: WebdriverIO's Tauri service starts and stops each scenario's app
// (E2E-INPUT); this configuration prepares their fixtures before and the runner closes them
// after (teardown).
import { SevereServiceError } from "webdriverio";
import { readCandidate } from "./build.ts";
import { selectedScenarios } from "./catalog.ts";
import { own, type Owned } from "./fixtures/owned.ts";
import { environment } from "./fixtures/repo.ts";
import { displayEnvironment, setVariable, stopTreeAt, variable, type Environment } from "./platform.ts";
import { asNativePath, embeddedPorts, exists, makeDirectory, nativePath, readTextIfExists, suitePath, writeJson } from "./portable.ts";
import { AGAIN, COMPLETED, type Context } from "./scenario.ts";

const candidate = readCandidate();
const selected = selectedScenarios(JSON.parse(variable("DESKTOP_TEST_BATCH") ?? "[]") as string[]);
if (!selected.length) throw new Error("a WDIO batch needs its scenarios in DESKTOP_TEST_BATCH");
const output = makeDirectory(asNativePath(variable("DESKTOP_TEST_BATCH_OUTPUT") ?? ""));
const owned: Owned[] = [];

/** Why preparing the batch failed, for the runner: WDIO wraps it. */
export let preparationFailure: unknown;

/** Fail unless every scenario of the batch ran to its end: a Launcher that found no spec, or a
 * worker that stopped before its test, ends without failing (E2E-EXPLICIT-FAILURE). */
export function assertCompleted(): void {
  const missing = selected.filter((_item, index) => !owned[index] || !exists(nativePath(owned[index].root, COMPLETED)));
  if (missing.length) throw new Error(`no completed run for ${missing.map((item) => item.id).join(", ")}`);
}

/** Close what each scenario started and delete its files; called once the service has
 * stopped every app of the batch. */
export async function teardown(): Promise<void> {
  const results = await Promise.allSettled(owned.splice(0).map((scope) => scope.close()));
  const failures = results.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
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
  specs: [suitePath("journey.spec.ts")],
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
      const ports = await embeddedPorts(selected.length);
      const unset = new Set<string>();
      for (const [index, item] of selected.entries()) {
        const scope = own(nativePath(candidate.root, "fixtures"));
        owned.push(scope);
        // An app the scenario started again, should its worker have ended before ending it.
        scope.defer(() => {
          const pid = Number(readTextIfExists(nativePath(scope.root, AGAIN)) ?? "");
          if (pid > 0) stopTreeAt(pid);
        });
        const input = await item.prepare(scope);
        const env = { ...environment(input.fixture), ...displayEnvironment(), ARTIFACTIZE_NO_UPDATE_CHECK: "1", ...input.env };
        for (const [name, value] of Object.entries(env)) if (value === undefined) unset.add(name);
        const context = nativePath(scope.root, "context.json");
        const launch = { app: input.app ?? candidate.app, args: ["--state-dir", input.fixture.state, "--reviewer", "alice", ...(input.args ?? [])], env: set(env) };
        writeJson(context, { ...input, root: scope.root, port: ports[index], launch } satisfies Context);
        capabilities[index]["artifactize:context"] = context;
        if (input.app) capabilities[index]["tauri:options"] = { application: input.app };
        capabilities[index]["wdio:tauriServiceOptions"] = { appArgs: launch.args, env: launch.env, embeddedPort: ports[index] };
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
  beforeSession(_config: unknown, capability: Capability) {
    setVariable("DESKTOP_TEST_SCENARIO", capability["artifactize:scenario"]);
    setVariable("DESKTOP_TEST_CONTEXT", capability["artifactize:context"]);
  },
  mochaOpts: { timeout: Math.max(...selected.map((item) => item.timeout ?? 180000)) },
};
