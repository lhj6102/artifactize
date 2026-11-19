// The upstream embedded service is the sole native app/driver lifecycle owner.
import { remote, SevereServiceError } from "webdriverio";
import { readCandidate } from "./build.ts";
import { AppStillAlive, assertAppsStopped, recordAdmission } from "./observation.ts";
import { companionPortFree, companionPortWanted, graphToolFailure, observeGraphTool } from "./graph-tool-session.ts";
import { observe } from "./domain-fixtures/observe.ts";
import { selectedScenarios } from "./catalog.ts";
import { createdRoots, environment, releaseFixtures } from "./fixture.ts";
import { browserEnvironment } from "./browser-fixtures.ts";
import { appsAtOnce, assertHarnessPlatform, claimForegroundWindow, reserveEmbeddedPorts, harnessEnvironment, assertClipboardDesktop, commandDirectory, placeApp, programRunning, replaceEnvironment, requiredVariable, setVariable, suitePath, variable, type Environment, writeJson } from "./platform.ts";
import { releaseClaudeLogins } from "./domain-fixtures/claude-code.ts";
import { asNativePath, claimMachineResource, embeddedReady, exists, makeDirectory, nativePath, writeText, type NativePath } from "./portable.ts";
import { completionMarker, type Prepared } from "./scenario.ts";

// Reject an unsafe OS session before any fixture or service is initialized.
assertHarnessPlatform();
const candidate = readCandidate();
const ids = JSON.parse(variable("DESKTOP_TEST_BATCH") ?? "[]") as string[];
if (!ids.length) throw new Error("a WDIO invocation requires an explicit scenario batch");
const selected = selectedScenarios(ids);
// The service launches every capability's app while it prepares, so a batch never holds more
// scenarios than the platform runs apps at once (E2E-ISOLATED); the suite batches that way.
if (selected.length > appsAtOnce) {
  throw new Error(`a batch holds at most ${appsAtOnce} scenario(s) on this platform, got ${selected.length}`);
}
const outputDir = makeDirectory(asNativePath(requiredVariable("DESKTOP_TEST_BATCH_OUTPUT", "each WDIO batch writes its own logs")));
const prepared: Prepared[] = [];
const companions = new Map<number, { port: number; program: NativePath; owner?: NativePath; observation?: Promise<void>; handoff?: () => Promise<void> }>();
const driverClaims: (() => Promise<void>)[] = [];
// WDIO wraps severe hook errors; retain the original for the matching runner owner.
export let preparationFailure: unknown;
export let ownerFiles: ReturnType<typeof recordAdmission> | undefined;

/** The product has one fixed auth callback endpoint. All its auth actions therefore
 * form one independent accounts capability; no unrelated capability consumes it. */
export const fixedResources = {
  accounts: { callbackHost: "127.0.0.1", callbackPort: 1455 },
  // The Tool log and release command use the clipboard through this one capability.
  "monitor-tool-log-scroll": { clipboard: true },
} as const;

/** The fixture roots of this batch's apps. */
export function preparedRoots(): NativePath[] {
  return prepared.map((input) => input.fixture.root);
}

/** Every scenario of this batch ran its phase to the end: a Launcher that found no spec, or
 * a worker that stopped before its test, returns without failing (E2E-EXPLICIT-FAILURE). */
export function assertCompleted(relaunch: boolean): void {
  const missing = selected.filter((_item, index) => {
    const input = prepared[index];
    return !input || !exists(nativePath(input.fixture.root, completionMarker(relaunch)));
  }).map((item) => item.id);
  if (missing.length) throw new Error(`no completed ${relaunch ? "relaunch" : "run"} for ${missing.join(", ")}`);
}

/** Called by the runner AFTER Launcher.run returns, never from config onComplete:
 * WDIO runs the user's onComplete before the upstream service's shutdown hook. */
export async function releasePrepared(): Promise<void> {
  const failures: unknown[] = [];
  // A missing birth record is not permission to remove that capability's inputs.
  // It must not strand unrelated fixtures whose native owners did stop.
  const roots = createdRoots();
  const retained = new Set<NativePath>();
  const release = async (action: () => Promise<void>) => {
    try { await action(); } catch (error) { failures.push(error); }
  };
  for (const [index, input] of prepared.splice(0).entries()) {
    const companion = companions.get(index);
    // The primary app's end is confirmed on its own; the companion's handoff and end are
    // confirmed whatever that check found.
    try {
      if (ownerFiles?.[index]) assertAppsStopped(candidate.observer, [ownerFiles[index]]);
    } catch (error) {
      failures.push(error);
      retained.add(input.fixture.root);
    }
    try {
      if (companion?.handoff) await release(companion.handoff);
      // The app that would launch the companion has ended: a companion it launched has written
      // its birth record. Without one, confirm that no companion program runs, since one may
      // have started and been stopped before it recorded itself.
      const launched = !!companion?.owner && exists(companion.owner);
      if (companion && !launched) {
        const program = companion.program;
        await observe(() => !programRunning(program),
          () => "a graph companion without a birth record is still running", 65000);
      }
      if (launched && companion?.observation) await release(() => companion.observation!);
      if (launched && companion?.owner) {
        const owner = companion.owner;
        // The test build's companion has a native close watchdog if attachment failed.
        // Do not report a routine deadline as success, or delete without the native join.
        await observe(() => {
          try { assertAppsStopped(candidate.observer, [owner]); return true; }
          catch (error) { if (error instanceof AppStillAlive) return false; throw error; }
        }, () => "native graph companion shutdown is unconfirmed after its close watchdog", 65000);
      }
    }
    catch (error) {
      // The run fails and the files stay for diagnosis; the suite's own servers and jobs
      // still close, since nothing would end them after the runner returns.
      failures.push(error);
      retained.add(input.fixture.root);
    }
    const before = failures.length;
    await release(() => releaseClaudeLogins(input.fixture.root));
    await release(async () => { await input.release?.(); });
    if (!retained.has(input.fixture.root)) await release(async () => { await input.restore?.(); });
    if (failures.length !== before) retained.add(input.fixture.root);
  }
  // An app whose end is unconfirmed may still use a once-per-machine resource: its claims stay
  // held until this runner exits rather than being handed to the next batch.
  const claims = driverClaims.splice(0);
  if (retained.size === 0) for (const releaseClaim of claims) await release(releaseClaim);
  await release(async () => { await releaseFixtures(roots, candidate, retained); });
  if (failures.length) throw new AggregateError(failures, "desktop scenario fixture release failed");
}

type Capability = {
  browserName: string;
  "artifactize:scenario": string;
  "artifactize:context"?: string;
  "wdio:specs": string[];
  "wdio:enforceWebDriverClassic": boolean;
  "wdio:tauriServiceOptions"?: Record<string, unknown>;
  "tauri:options"?: { application: string };
};

export const config = {
  runner: "local",
  specs: [suitePath("journey.spec.ts")],
  framework: "mocha",
  reporters: ["spec"],
  logLevel: "warn",
  outputDir,
  maxInstances: selected.length,
  maxInstancesPerCapability: 1,
  autoXvfb: false,
  bail: 0,
  specFileRetries: 0,
  connectionRetryCount: 0,
  waitforTimeout: 15000,
  capabilities: selected.map((item): Capability => ({
    browserName: "tauri",
    "artifactize:scenario": item.id,
    "wdio:specs": [suitePath("journey.spec.ts")],
    "wdio:enforceWebDriverClassic": true,
  })),
  services: [["tauri", {
    appBinaryPath: candidate.app,
    driverProvider: "embedded",
    startTimeout: 60000,
    logDir: outputDir,
    captureBackendLogs: true,
  }]],
  async onPrepare(_config: unknown, capabilities: Capability[]) {
    try {
      assertHarnessPlatform();
      const native = harnessEnvironment();
      const relaunch = variable("DESKTOP_TEST_RELAUNCH") === "1";
      // A Windows desktop's one foreground window is this batch's while its apps run; a
      // relaunch keeps the claims of its first launch.
      if (!relaunch) driverClaims.push(await claimForegroundWindow());
      for (let index = 0; index < selected.length; index++) {
        const item = selected[index];
        if (item.id === "monitor-tool-log-scroll" && fixedResources[item.id].clipboard) {
          assertClipboardDesktop();
          if (!relaunch) driverClaims.push(await claimMachineResource("clipboard"));
        }
        if (item.id === "accounts" && !relaunch) driverClaims.push(await claimMachineResource("auth-callback"));
        const input = relaunch ? prepared[index] : await item.prepare();
        if (!input) throw new Error("the relaunched case lost its owning fixture");
        if (!relaunch) prepared.push(input);
        const context = nativePath(input.fixture.root, "worker-context.json");
        const args = ["--state-dir", input.fixture.state, "--reviewer", "alice", ...(input.args ?? [])];
        let env: Environment = {
          ...environment(input.fixture), ...native,
          ARTIFACTIZE_NO_UPDATE_CHECK: input.env?.ARTIFACTIZE_RELEASES_URL ? undefined : "1", ...input.env,
          DESKTOP_TEST_SCENARIO: item.id, DESKTOP_TEST_FIXTURE_ROOT: input.fixture.root,
          DESKTOP_TEST_RELAUNCH: variable("DESKTOP_TEST_RELAUNCH"),
        };
        if (item.id === "current-human-graph-tool") {
          const bin = nativePath(input.fixture.root, "graph-tool-bin");
          const app = placeApp(candidate.app, bin);
          const [endpoint] = await reserveEmbeddedPorts(1);
          driverClaims.push(endpoint.release);
          // The listener stays until the case is about to click the graph tool (it asks through
          // a file); the claim stays until teardown.
          const fixture = input.fixture;
          let polling: ReturnType<typeof setInterval> | undefined;
          // Teardown settles the handoff too, also when the case never asked for the port.
          let settle = () => {};
          const handoff = new Promise<void>((done, fail) => {
            settle = done;
            polling = setInterval(() => {
              if (!exists(companionPortWanted(fixture))) return;
              clearInterval(polling);
              endpoint.close().then(() => { writeText(companionPortFree(fixture), ""); done(); }, fail);
            }, 50);
          });
          void handoff.catch(() => {});
          companions.set(index, { port: endpoint.port, program: app, handoff: async () => {
            clearInterval(polling);
            settle();
            await endpoint.close();
            await handoff;
          } });
          env = { ...commandDirectory(env, bin), ARTIFACTIZE_STATE_HOME: input.fixture.state, ARTIFACTIZE_E2E_COMPANION_EXE: app,
            ARTIFACTIZE_E2E_COMPANION_PORT: String(endpoint.port) };
        }
        const entry = input.browser;
        if (entry && !relaunch) {
          const browser = await browserEnvironment(input.fixture.root, env, entry);
          env = browser.env;
          // Runs after the app has ended and before the fixture is deleted.
          const release = input.release;
          input.release = async () => {
            const failures: unknown[] = [];
            try { await browser.release?.(); } catch (error) { failures.push(error); }
            try { await release?.(); } catch (error) { failures.push(error); }
            if (failures.length) throw new AggregateError(failures, "browser fixtures could not be released");
          };
        }
        // Release callbacks and native fixture handles never cross the worker boundary.
        writeJson(context, { fixture: input.fixture, args, env, data: input.data }, { private: true });
        capabilities[index]["artifactize:context"] = context;
        if (input.app) capabilities[index]["tauri:options"] = { application: input.app };
        capabilities[index]["wdio:tauriServiceOptions"] = {
          appArgs: args,
          env: Object.fromEntries(Object.entries(env).filter((entry): entry is [string, string] => entry[1] !== undefined)),
        };
      }
      const ownerRoot = nativePath(candidate.root, "runner-environment");
      const owner = { root: ownerRoot, repo: nativePath(ownerRoot, "repo"), state: nativePath(ownerRoot, "state") };
      const safe = { ...environment(owner), ...native };
      // The service merges process.env before per-capability env: absent secrets and
      // explicitly unset per-case options must not reappear from the launcher.
      for (const input of prepared) for (const [name, value] of Object.entries(input.env ?? {})) {
        if (value === undefined) delete safe[name];
      }
      replaceEnvironment(safe);
      // Bind only after every fixture has its own endpoints. The per-port runner claim
      // survives the handoff while the native service acquires its own listener.
      const endpoints = await reserveEmbeddedPorts(selected.length);
      driverClaims.push(...endpoints.map((endpoint) => endpoint.release));
      try {
        for (const [index, endpoint] of endpoints.entries()) {
          capabilities[index]["wdio:tauriServiceOptions"]!.embeddedPort = endpoint.port;
        }
      } finally {
        await Promise.all(endpoints.map((endpoint) => endpoint.close()));
      }
      const hasCompanion = companions.size > 0;
      const capabilityIds = selected.map((item) => item.id);
      const recorded = recordAdmission(candidate.root, hasCompanion ? [...capabilityIds, "graph-tool-companion"] : capabilityIds);
      ownerFiles = recorded.slice(0, selected.length);
      for (const [index, capability] of capabilities.entries()) {
        const env = capability["wdio:tauriServiceOptions"]!.env as Environment;
        env.ARTIFACTIZE_E2E_OWNER_FILE = ownerFiles[index];
        const companion = companions.get(index);
        if (companion) {
          companion.owner = recorded[selected.length];
          env.ARTIFACTIZE_E2E_COMPANION_OWNER = companion.owner;
        }
      }
    } catch (cause) {
      preparationFailure = cause;
      // WDIO logs and swallows ordinary launcher-hook failures. Its public severe
      // error is required to stop before the service launches any unprepared app.
      const failure = new SevereServiceError(`Desktop scenario preparation failed: ${cause instanceof Error ? cause.message : String(cause)}`);
      failure.cause = cause;
      throw failure;
    }
  },
  onWorkerStart(_cid: string, capability: Capability) {
    const index = selected.findIndex((item) => item.id === capability["artifactize:scenario"]);
    const companion = companions.get(index);
    if (!companion?.owner) return;
    const { fixture } = prepared[index];
    const { owner, port } = companion;
    // The app is launched by the real Human tool, not by this observation session.
    companion.observation = (async () => {
      await observe(() => exists(owner) ? embeddedReady(port) : false,
        () => "the product-launched graph companion did not bring up its WebDriver endpoint");
      const window = await remote({ hostname: "127.0.0.1", port, path: "/", logLevel: "warn",
        capabilities: { browserName: "tauri", "wdio:enforceWebDriverClassic": true },
        connectionRetryCount: 0, connectionRetryTimeout: 5000 });
      await observeGraphTool(fixture, window);
    })();
    void companion.observation.catch((error: unknown) => writeText(graphToolFailure(fixture), String(error))).catch(() => {});
  },
  beforeSession(_config: unknown, capabilities: Capability) {
    const context = capabilities["artifactize:context"];
    if (!context) throw new Error("scenario context was not prepared before the app launch");
    setVariable("DESKTOP_TEST_SCENARIO", capabilities["artifactize:scenario"]);
    setVariable("DESKTOP_TEST_CONTEXT", context);
  },
  mochaOpts: { timeout: selected.some((item) => item.id === "settings-claude-timeout") ? 660000 : 120000 },
};
