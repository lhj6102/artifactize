// The upstream embedded service is the sole native app/driver lifecycle owner.
import { SevereServiceError } from "webdriverio";
import { readCandidate } from "./build.ts";
import { selectedScenarios } from "./catalog.ts";
import { environment, releaseFixtures, settleFixtures } from "./fixture.ts";
import {
  browserEnvironment, asNativePath, releaseBrowser, assertHarnessPlatform, assertRunnerMachine, embeddedPorts, harnessEnvironment,
  assertClipboardDesktop, makeDirectory, nativePath, replaceEnvironment, requiredVariable, setVariable, suitePath, variable, writeJson,
  type Environment,
} from "./platform.ts";
import type { Prepared } from "./scenario.ts";

// Reject an unsafe OS session before any fixture or service is initialized.
assertRunnerMachine();
const candidate = readCandidate();
const ids = JSON.parse(variable("DESKTOP_TEST_BATCH") ?? "[]") as string[];
if (!ids.length) throw new Error("a WDIO invocation requires an explicit scenario batch");
const selected = selectedScenarios(ids);
const outputDir = makeDirectory(asNativePath(requiredVariable("DESKTOP_TEST_BATCH_OUTPUT", "each WDIO batch writes its own logs")));
const prepared: Prepared[] = [];
// WDIO wraps severe hook errors; retain the original for the matching runner owner.
export let preparationFailure: unknown;

/** The product has one fixed auth callback endpoint. All its auth actions therefore
 * form one independent accounts capability; no unrelated capability consumes it. */
export const fixedResources = {
  accounts: { callbackHost: "127.0.0.1", callbackPort: 1455 },
  // This is the only capability to copy/paste. It writes its own selection before reading
  // it back through a paste; neither the runner nor another capability reads the clipboard.
  "monitor-tool-log-scroll": { clipboard: true },
} as const;

/** Called by the runner AFTER Launcher.run returns, never from config onComplete:
 * WDIO runs the user's onComplete before the upstream service's shutdown hook. */
/** Called by the runner when an app was not confirmed stopped: its inputs stay, but the
 * Runs the fixtures started are cancelled and joined, so no fixture worker outlives the batch. */
export function settlePrepared(): void {
  settleFixtures();
}

export async function releasePrepared(): Promise<void> {
  const failures: unknown[] = [];
  for (const input of prepared.splice(0)) {
    try { await input.release?.(); } catch (error) { failures.push(error); }
  }
  try { await releaseFixtures(); } catch (error) { failures.push(error); }
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
      const endpoints = await embeddedPorts(selected.length);
      for (let index = 0; index < selected.length; index++) {
        const item = selected[index];
        if (item.id === "monitor-tool-log-scroll") assertClipboardDesktop();
        const input = variable("DESKTOP_TEST_RELAUNCH") === "1" ? prepared[index] : await item.prepare();
        if (!input) throw new Error("the relaunched case lost its owning fixture");
        if (variable("DESKTOP_TEST_RELAUNCH") !== "1") prepared.push(input);
        const context = nativePath(outputDir, `${item.id}.json`);
        const args = ["--state-dir", input.fixture.state, "--reviewer", "alice", ...(input.args ?? [])];
        let env: Environment = {
          ...environment(input.fixture), ...native, ...input.env,
          DESKTOP_TEST_SCENARIO: item.id, DESKTOP_TEST_FIXTURE_ROOT: input.fixture.root,
          DESKTOP_TEST_RELAUNCH: variable("DESKTOP_TEST_RELAUNCH"),
          ARTIFACTIZE_E2E_OWNER_FILE: nativePath(outputDir, `${item.id}.owner.json`),
        };
        const entry = input.browser;
        if (entry) {
          env = browserEnvironment(input.fixture.root, env, entry);
          // Runs after the app has ended and before the fixture is deleted.
          const release = input.release;
          input.release = async () => {
            const failures: unknown[] = [];
            try { await releaseBrowser(input.fixture.root, entry); } catch (error) { failures.push(error); }
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
          embeddedPort: endpoints[index],
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
    } catch (cause) {
      preparationFailure = cause;
      // WDIO logs and swallows ordinary launcher-hook failures. Its public severe
      // error is required to stop before the service launches any unprepared app.
      const failure = new SevereServiceError(`Desktop scenario preparation failed: ${cause instanceof Error ? cause.message : String(cause)}`);
      failure.cause = cause;
      throw failure;
    }
  },
  beforeSession(_config: unknown, capabilities: Capability) {
    const context = capabilities["artifactize:context"];
    if (!context) throw new Error("scenario context was not prepared before the app launch");
    setVariable("DESKTOP_TEST_SCENARIO", capabilities["artifactize:scenario"]);
    setVariable("DESKTOP_TEST_CONTEXT", context);
  },
  mochaOpts: { timeout: 120000 },
};
