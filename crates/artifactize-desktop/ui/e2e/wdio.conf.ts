// The upstream embedded service is the sole native app/driver lifecycle owner.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readCandidate } from "./build.ts";
import { selectedScenarios } from "./catalog.ts";
import { environment, releaseFixtures } from "./fixture.ts";
import { accountBrowserEnvironment, assertHarnessPlatform, embeddedPorts, harnessEnvironment } from "./platform.ts";
import type { Prepared } from "./scenario.ts";

const candidate = readCandidate();
const ids = JSON.parse(process.env.DESKTOP_TEST_BATCH ?? "[]") as string[];
if (!ids.length) throw new Error("a WDIO invocation requires an explicit scenario batch");
const selected = selectedScenarios(ids);
const outputDir = process.env.DESKTOP_TEST_BATCH_OUTPUT;
if (!outputDir) throw new Error("DESKTOP_TEST_BATCH_OUTPUT is required");
mkdirSync(outputDir, { recursive: true });
const prepared: Prepared[] = [];

/** The product has one fixed auth callback endpoint. All its auth actions therefore
 * form one independent accounts capability; no unrelated capability consumes it. */
export const fixedResources = { accounts: { callbackHost: "127.0.0.1", callbackPort: 1455 } } as const;

/** Called by the runner AFTER Launcher.run returns, never from config onComplete:
 * WDIO runs the user's onComplete before the upstream service's shutdown hook. */
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
};

export const config = {
  runner: "local",
  specs: [join(import.meta.dirname, "journey.spec.ts")],
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
    "wdio:specs": [join(import.meta.dirname, "journey.spec.ts")],
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
    assertHarnessPlatform();
    const native = harnessEnvironment();
    const endpoints = await embeddedPorts(selected.length);
    for (let index = 0; index < selected.length; index++) {
      const item = selected[index];
      const input = await item.prepare();
      prepared.push(input);
      const context = join(outputDir, `${item.id}.json`);
      const args = ["--state-dir", input.fixture.state, "--reviewer", "alice", ...(input.args ?? [])];
      let env: NodeJS.ProcessEnv = {
        ...environment(input.fixture), ...native, ...input.env,
        DESKTOP_TEST_SCENARIO: item.id, DESKTOP_TEST_FIXTURE_ROOT: input.fixture.root,
      };
      if (item.id === "accounts") env = accountBrowserEnvironment(input.fixture, env);
      const serialized = JSON.stringify({ fixture: input.fixture, args, env, data: input.data });
      // Release callbacks and native fixture handles never cross the worker boundary.
      writeFileSync(context, serialized, { mode: 0o600 });
      capabilities[index]["artifactize:context"] = context;
      capabilities[index]["wdio:tauriServiceOptions"] = {
        appArgs: args,
        env: Object.fromEntries(Object.entries(env).filter((entry): entry is [string, string] => entry[1] !== undefined)),
        embeddedPort: endpoints[index],
      };
    }
    const ownerRoot = join(candidate.root, "runner-environment");
    const owner = { root: ownerRoot, repo: join(ownerRoot, "repo"), state: join(ownerRoot, "state") };
    const safe = { ...environment(owner), ...native };
    // The service merges process.env before per-capability env: absent secrets and
    // explicitly unset per-case options must not reappear from the launcher.
    for (const input of prepared) for (const [name, value] of Object.entries(input.env ?? {})) {
      if (value === undefined) delete safe[name];
    }
    for (const name of Object.keys(process.env)) if (!(name in safe)) delete process.env[name];
    Object.assign(process.env, safe);
  },
  beforeSession(_config: unknown, capabilities: Capability) {
    const context = capabilities["artifactize:context"];
    if (!context) throw new Error("scenario context was not prepared before the app launch");
    process.env.DESKTOP_TEST_SCENARIO = capabilities["artifactize:scenario"];
    process.env.DESKTOP_TEST_CONTEXT = context;
  },
  mochaOpts: { timeout: 120000 },
};
