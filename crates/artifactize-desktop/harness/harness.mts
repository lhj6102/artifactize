// The desktop app as an Agent tool session (declared in ../index.artf).
// start wraps the whole harness in the platform's standard headless environment;
// serve delegates app provisioning, launch and shutdown to @wdio/tauri-service.
// Tools communicate over loopback TCP, using the private session directory's token.
// An explicit DESKTOP_TEST_CANDIDATE is snapshotted once, never found or rebuilt.
import { createConnection, createServer, type Socket } from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { create, environment, releaseFixtures, type Fixture } from "../ui/e2e/fixture.ts";
import { assertHarnessPlatform, embeddedPorts, harnessEnvironment, runnerCommand } from "../ui/e2e/platform.ts";
import { page, press, typeText } from "../ui/e2e/keys.ts";
import { cargoGate, cleanup, snapshotCandidate, type Candidate } from "../ui/e2e/build.ts";
import { checkpoint, commandsRendered, type Checkpoint } from "../ui/e2e/commands.ts";

// Resolve the public package entry from the frontend which declares this dependency.
const service = createRequire(new URL("../ui/package.json", import.meta.url))("@wdio/tauri-service") as
  typeof import("../ui/node_modules/@wdio/tauri-service");
const { startWdioSession, createTauriCapabilities, cleanupWdioSession } = service;
const LAUNCH_TIMEOUT = 60000;
const REQUEST_TIMEOUT = 45 * 60000; // Includes the standalone API's bounded startup retries.
const ENDPOINT = "control.json";
type Session = { browser: WebdriverIO.Browser; stop: () => Promise<void> };
type Endpoint = { port: number; token: string };
const sessionDir = process.env.ARTIFACTIZE_SESSION_DIR;

type Request =
  | { op: "snapshot" }
  | { op: "click"; element: number }
  | { op: "type"; element?: number; text: string }
  | { op: "key"; key: string }
  | { op: "cli"; args: string[] }
  | { op: "launch"; screen: "monitor" | "review" | "graph" }
  | { op: "change-declaration" }
  | { op: "stop" };

// WebDriver's element references, kept outside the page, retain identity across reorders.
// A removed control is stale; its number must never target its replacement at the same index.
const controls = new WeakMap<WebdriverIO.Browser, WebdriverIO.Element[]>();
const CONTROL_SELECTOR = "a, button, input, textarea, select, summary, [role], [tabindex], [aria-label]";

/** The window as text: its focus, every visible control with a number to act on, then its text. */
export async function snapshot(browser: WebdriverIO.Browser): Promise<string> {
  // Embedded WebDriver accepts element arguments but does not serialize DOM-node returns.
  const candidates = await browser.$$(CONTROL_SELECTOR).getElements();
  const shown = await browser.execute((references: HTMLElement[]) => {
    const indices: number[] = [];
    const visible = (element: Element) => {
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return box.width > 0 && box.height > 0 && style.visibility !== "hidden" && style.display !== "none";
    };
    const name = (element: Element) =>
      element.getAttribute("aria-label") ??
      (element as HTMLElement).innerText?.replace(/\s+/g, " ").trim().slice(0, 80) ??
      "";
    const lines: string[] = [];
    let count = 0;
    for (const [index, element] of references.entries()) {
      if (!element.isConnected) throw new Error("snapshot control became stale; take a new snapshot");
      if (!visible(element)) continue;
      const role = element.getAttribute("role") ?? element.tagName.toLowerCase();
      count += 1;
      indices.push(index);
      const states = [
        ["selected", "aria-selected"],
        ["expanded", "aria-expanded"],
        ["checked", "aria-checked"],
        ["pressed", "aria-pressed"],
        ["current", "aria-current"],
      ]
        .map(([label, attribute]) => (element.hasAttribute(attribute) ? `${label}=${element.getAttribute(attribute)}` : ""))
        .filter(Boolean);
      if ((element as HTMLButtonElement).disabled || element.getAttribute("aria-disabled") === "true") states.push("disabled");
      if (element === document.activeElement) states.push("focused");
      const value = (element as HTMLInputElement).value;
      if (typeof value === "string" && element.matches("input, textarea, select")) states.push(`value=${JSON.stringify(value)}`);
      lines.push(`[${count}] ${role} ${JSON.stringify(name(element))}${states.length ? ` (${states.join(", ")})` : ""}`);
    }
    return { text: `controls:\n${lines.join("\n")}\n\ntext:\n${document.body.innerText}`, indices };
  }, candidates as unknown as HTMLElement[]);
  // ID-only selectors cannot refetch by DOM position and retarget a replacement control.
  controls.set(browser, await Promise.all(shown.indices.map((index) => browser.$({
    "element-6066-11e4-a52e-4f735466cecf": candidates[index].elementId,
  }).getElement())));
  return shown.text;
}

/** Native completion identifies the current search even before its debounce dispatches. */
async function settled(browser: WebdriverIO.Browser, before?: Checkpoint): Promise<string> {
  await commandsRendered(browser, before, LAUNCH_TIMEOUT);
  return snapshot(browser);
}

/** A launch is ready when the fixture's actual entry-point content has rendered. */
async function rendered(browser: WebdriverIO.Browser, screen: "monitor" | "review" | "graph") {
  await browser.waitUntil(() => browser.execute((entry) => {
    if (entry === "graph") return !!document.querySelector('.svelte-flow__node[data-id="a:release"]');
    const selector = entry === "review" ? '[aria-label="Requests"] [role="option"]' : '[role="treeitem"]';
    return [...document.querySelectorAll(selector)].some((row) => row.textContent?.includes("signoff"));
  }, screen), { timeout: LAUNCH_TIMEOUT, timeoutMsg: `${screen} fixture content did not render` });
}

export async function target(browser: WebdriverIO.Browser, element: number) {
  const found = controls.get(browser)?.[element - 1];
  if (!Number.isInteger(element) || element < 1 || !found || !(await found.isExisting()) || !(await found.isDisplayed())) {
    throw new Error(`no element [${element}] from the last snapshot remains visible; take a new snapshot`);
  }
  return found;
}

/** Keys as `Control+k` or `Enter`: modifiers first, one action key last. */
async function pressKey(browser: WebdriverIO.Browser, key: string): Promise<void> {
  const keys = key.split("+");
  const seen = page(browser);
  const before = (await seen.received()).length;
  await press(browser, keys, async () => (await seen.received()).length > before, `key ${key}`, { wait: 1000 });
}

/** The service merges process.env into the app environment, so sanitize that source too. */
async function launch(candidate: Candidate, fixture: Fixture, platformEnv: Record<string, string>, args: string[] = []): Promise<Session> {
  const env = { ...environment(fixture), ...platformEnv };
  for (const name of Object.keys(process.env)) if (env[name] === undefined) delete process.env[name];
  for (const [name, value] of Object.entries(env)) if (value !== undefined) process.env[name] = value;
  const capabilities = createTauriCapabilities(candidate.app, {
    appArgs: [...args, "--state-dir", fixture.state, "--reviewer", "alice"],
    driverProvider: "embedded",
    startTimeout: LAUNCH_TIMEOUT,
    commandTimeout: LAUNCH_TIMEOUT,
    logLevel: "error",
  });
  capabilities["wdio:enforceWebDriverClassic"] = true;
  const [embeddedPort] = await embeddedPorts(1);
  const serviceOptions = {
    ...capabilities["wdio:tauriServiceOptions"],
    embeddedPort,
    logDir: join(candidate.root, "logs"),
  };
  capabilities["wdio:tauriServiceOptions"] = serviceOptions;
  const browser = await startWdioSession(capabilities);
  // Commands must fail within one attempt rather than replaying actions after transport loss.
  browser.options.connectionRetryTimeout = LAUNCH_TIMEOUT;
  browser.options.connectionRetryCount = 0;
  let ending: Promise<void> | undefined;
  return { browser, stop: () => ending ??= cleanupWdioSession(browser) };
}

async function handle(candidate: Candidate, session: Session, fixture: Fixture, request: Request): Promise<unknown> {
  const { browser } = session;
  const before = await checkpoint(browser);
  switch (request.op) {
    case "snapshot":
      return { text: await settled(browser, before) };
    case "click":
      await (await target(browser, request.element)).click();
      return { text: await settled(browser, before) };
    case "type":
      if (request.element !== undefined) await (await target(browser, request.element)).click();
      await typeText(browser, request.text);
      return { text: await settled(browser, before) };
    case "key":
      await pressKey(browser, request.key);
      return { text: await settled(browser, before) };
    case "cli": {
      const done = spawnSync(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, ...request.args], {
        env: environment(fixture),
        encoding: "utf8",
        timeout: 60000,
      });
      if (done.error) throw done.error;
      return { exitCode: done.status, stdout: done.stdout, stderr: done.stderr };
    }
    default:
      throw new Error("unknown operation");
  }
}

/** Close admission immediately, then release the latest owner after the active command settles. */
export function commandStream(close: () => void, release: () => Promise<void>) {
  let pending = Promise.resolve();
  let ending: Promise<void> | undefined;
  return {
    run<T>(operation: () => Promise<T>): Promise<T> {
      if (ending) return Promise.reject(new Error("the app session is stopping"));
      const result = pending.then(() => {
        if (ending) throw new Error("the app session is stopping");
        return operation();
      });
      // A failed command must not prevent shutdown from releasing its replacement owner.
      pending = result.then(() => {}, () => {});
      return result;
    },
    end(): Promise<void> {
      if (!ending) {
        ending = pending.then(release);
        close();
      }
      return ending;
    },
  };
}

/** Preserve the operation's failure as well as any failed cleanup, including in tool replies. */
async function throwAfterCleanup(error: unknown, release: () => Promise<void>): Promise<never> {
  try { await release(); }
  catch (shutdown) { throw new AggregateError([error, shutdown], "operation and cleanup failed"); }
  throw error;
}

function errorText(error: unknown): string {
  if (error instanceof AggregateError) return `${error.message}: ${error.errors.map(errorText).join("; ")}`;
  return error instanceof Error ? error.message : String(error);
}

/** The Agent protocol treats stdout's first line as readiness, never a driver log. */
export function forwardReadiness(source: NodeJS.ReadableStream, output: NodeJS.WritableStream, diagnostics: NodeJS.WritableStream) {
  let ready = false;
  const lines = createInterface({ input: source });
  lines.on("line", (line) => {
    if (line === "ready" && !ready) {
      ready = true;
      output.write("ready\n");
    } else diagnostics.write(line + "\n");
  });
  return lines;
}

/** Forward termination through the standard wrappers' inherited stdin, not their
 * PIDs: killing an intermediate wrapper can remove X11 before service cleanup. */
export async function superviseOwner(command: ReturnType<typeof runnerCommand>): Promise<void> {
  const child = spawn(command.program, command.args, { env: command.env, stdio: ["pipe", "pipe", "inherit"] });
  const failures: unknown[] = [];
  let stopping = false;
  const requestStop = () => {
    if (stopping) return;
    stopping = true;
    child.stdin!.end();
  };
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, requestStop);
  child.stdin!.on("error", (error) => { failures.push(error); });
  forwardReadiness(child.stdout!, process.stdout, process.stderr);
  try {
    await new Promise<void>((done) => {
      child.once("error", (error) => { failures.push(error); });
      child.once("close", (code, signal) => {
        if (code !== 0) failures.push(new Error(`desktop harness exited ${signal ?? code}`));
        done();
      });
    });
    if (failures.length) throw new AggregateError(failures, "desktop owner supervision failed");
  } finally {
    for (const signal of ["SIGINT", "SIGTERM"] as const) process.off(signal, requestStop);
  }
}

/** Standard wrappers own the display and bus for the entire service host process. */
async function start(): Promise<void> {
  await superviseOwner(runnerCommand([fileURLToPath(import.meta.url), "serve"]));
}

async function serve(): Promise<void> {
  if (!sessionDir) throw new Error("ARTIFACTIZE_SESSION_DIR is not set; run this as a tool session.");
  assertHarnessPlatform();
  const platformEnv = harnessEnvironment();
  process.chdir(sessionDir);
  let candidate: Candidate | undefined;
  let shown: { session: Session; fixture: Fixture } | undefined;
  let fixture: Fixture;
  let stopping = false;
  // Service cleanup completes before public Run settlement or input deletion.
  // Startup is an active command too, so an in-flight acquisition is joined first.
  const release = async () => {
    if (server.listening) server.close();
    await shown?.session.stop();
    shown = undefined;
    if (candidate) {
      await releaseFixtures();
      await cleanup(candidate.root);
    }
    rmSync(join(sessionDir, ENDPOINT), { force: true });
  };
  let edits = 0;
  let graphLaunches = 0;
  const commands = commandStream(() => { stopping = true; }, release);
  const end = () => commands.end();
  let finishStop!: (result: { error?: unknown }) => void;
  const stopped = new Promise<{ error?: unknown }>((done) => { finishStop = done; });
  const requestStop = () => void end().then(
    () => finishStop({}),
    (error: unknown) => finishStop({ error }),
  );
  const token = randomBytes(32).toString("hex");
  const server = createServer((socket: Socket) => {
    socket.setTimeout(REQUEST_TIMEOUT, () => socket.destroy(new Error("desktop tool request timed out")));
    const lines = createInterface({ input: socket });
    socket.once("close", () => lines.close());
    socket.once("error", () => lines.close());
    lines.once("line", (line) => {
      lines.close();
      void (async () => {
        let reply: unknown;
        try {
          const envelope = JSON.parse(line) as { token: string; request: Request };
          if (envelope.token !== token) throw new Error("invalid desktop session token");
          const request = envelope.request;
          if (request.op === "stop") {
            try {
              await end();
              socket.end(JSON.stringify({ stopped: true }) + "\n", () => finishStop({}));
            } catch (error) {
              socket.end(JSON.stringify({ error: errorText(error) }) + "\n", () => finishStop({ error }));
            }
            return;
          }
          reply = await commands.run(async () => {
            const input = candidate;
            if (!input) throw new Error("the app session has no prepared candidate");
            if (request.op === "launch") {
              if (!["monitor", "review", "graph"].includes(request.screen)) throw new Error("unknown desktop launch screen");
              await shown?.session.stop();
              shown = undefined;
              const target = request.screen === "graph" ? { ...fixture, state: join(fixture.root, `graph-state-${++graphLaunches}`) } : fixture;
              const args = request.screen === "graph" ? ["graph", "--repo", target.repo] : request.screen === "review" ? ["review"] : [];
              const session = await launch(input, target, platformEnv, args);
              // Accept the replacement before rendering can fail or shutdown can drain us.
              shown = { session, fixture: target };
              try {
                await rendered(session.browser, request.screen);
                return { text: await settled(session.browser) };
              } catch (error) {
                return throwAfterCleanup(error, async () => {
                  await session.stop();
                  shown = undefined;
                });
              }
            } else if (request.op === "change-declaration") {
              const path = join(fixture.repo, "docs", "index.artf");
              const title = `Docs criterion revision ${++edits}`;
              writeFileSync(path, readFileSync(path, "utf8").replace(/title = "[^"]*"/, `title = "${title}"`));
              return { text: `Changed docs/lint title to ${title}. The app has not been refreshed.` };
            } else {
              if (!shown) throw new Error("no app session is running; launch an entry point first");
              return handle(input, shown.session, shown.fixture, request);
            }
          });
        } catch (error) {
          reply = { error: errorText(error) };
        }
        socket.end(JSON.stringify(reply) + "\n");
      })().catch((error: Error) => { socket.destroy(error); });
    });
  });
  // Install before the first asynchronous acquisition: endpoint creation is not a
  // prerequisite for shutdown. EOF also covers an unexpectedly lost start wrapper.
  for (const signal of ["SIGTERM", "SIGINT"] as const) process.on(signal, requestStop);
  process.stdin.once("end", requestStop);
  process.stdin.resume();
  let startupStarted = false;
  try {
    await commands.run(async () => {
      startupStarted = true;
      candidate = await snapshotCandidate(sessionDir);
      if (stopping) return;
      fixture = await create();
      if (stopping) return;
      const session = await launch(candidate, fixture, platformEnv);
      shown = { session, fixture };
      if (stopping) return;
      await rendered(session.browser, "monitor");
      if (stopping) return;
      await new Promise<void>((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", () => { server.off("error", fail); done(); });
      });
      if (stopping) return;
      const address = server.address();
      if (!address || typeof address === "string") throw new Error("desktop control server has no TCP endpoint");
      writeFileSync(join(sessionDir, ENDPOINT), JSON.stringify({ port: address.port, token } satisfies Endpoint), { mode: 0o600, flag: "wx" });
      console.log("ready");
    });
    const result = await stopped;
    if ("error" in result) throw result.error;
  } catch (error) {
    if (stopping && !startupStarted) { await end(); return; }
    return throwAfterCleanup(error, end);
  } finally {
    for (const signal of ["SIGTERM", "SIGINT"] as const) process.off(signal, requestStop);
    process.stdin.off("end", requestStop);
    process.stdin.pause();
  }
}

/** One request to this session's loopback endpoint on every supported socket platform. */
function ask(request: Request): Promise<Record<string, unknown>> {
  if (!sessionDir) throw new Error("ARTIFACTIZE_SESSION_DIR is not set; run this as a tool session.");
  const endpoint = JSON.parse(readFileSync(join(sessionDir, ENDPOINT), "utf8")) as Endpoint;
  if (!Number.isInteger(endpoint.port) || endpoint.port < 1 || endpoint.port > 65535 || typeof endpoint.token !== "string") {
    throw new Error("invalid desktop session endpoint");
  }
  return new Promise((done, fail) => {
    const socket = createConnection({ host: "127.0.0.1", port: endpoint.port });
    socket.setTimeout(REQUEST_TIMEOUT, () => socket.destroy(new Error("desktop tool request timed out")));
    let text = "";
    socket.on("data", (chunk) => (text += chunk));
    socket.once("error", fail);
    socket.once("close", () => {
      try {
        done(JSON.parse(text) as Record<string, unknown>);
      } catch {
        fail(new Error("the app session ended without answering"));
      }
    });
    socket.write(JSON.stringify({ token: endpoint.token, request }) + "\n");
  });
}

async function input(): Promise<Record<string, unknown>> {
  let text = "";
  for await (const chunk of process.stdin) text += chunk;
  const envelope = JSON.parse(text) as { args?: Record<string, unknown> };
  return envelope.args ?? {};
}

function content(text: string, error = false) {
  console.log(JSON.stringify({ isError: error || undefined, content: [{ type: "text", text }] }));
}

if (import.meta.main) {
  const mode = process.argv[2];
  if (mode === "clippy" || mode === "test") {
    await cargoGate(mode);
  } else if (mode === "start") {
    await start();
  } else if (mode === "serve") {
    await serve();
    process.exit(0);
  } else if (mode === "stop") {
    const reply = await ask({ op: "stop" });
    if (typeof reply.error === "string") throw new Error(reply.error);
    if (reply.stopped !== true) throw new Error("the app session did not acknowledge cleanup");
  } else {
    const args = await input();
    const request: Request =
      mode === "snapshot"
        ? { op: "snapshot" }
        : mode === "cli"
          ? { op: "cli", args: args.args as string[] }
          : mode === "launch"
            ? { op: "launch", screen: args.screen as "monitor" | "review" | "graph" }
            : mode === "change-declaration"
              ? { op: "change-declaration" }
              : args.action === "click"
                ? { op: "click", element: args.element as number }
                : args.action === "type"
                  ? { op: "type", element: args.element as number | undefined, text: args.text as string }
                  : { op: "key", key: args.key as string };
    const reply = await ask(request);
    if (typeof reply.error === "string") content(reply.error, true);
    else if (mode === "cli") content(`exit ${reply.exitCode}\n--- stdout\n${reply.stdout}\n--- stderr\n${reply.stderr}`, reply.exitCode !== 0);
    else content(String(reply.text));
  }
}
