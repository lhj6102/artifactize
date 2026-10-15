// Drive the real Linux WebKitGTK app through tauri-driver and WebKitWebDriver.
// Required Linux capabilities: procfs, Xvfb, dbus-run-session, tauri-driver and
// WebKitWebDriver. These helpers own X11, session-bus and process-group setup;
// Windows and macOS use different native automation boundaries, not this harness.
import { createServer } from "node:net";
import { delimiter, join } from "node:path";
import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, readlinkSync, writeFileSync } from "node:fs";
import { remote } from "webdriverio";
import { app, environment, type Fixture } from "./fixture.ts";
import { identity, removeFixture, running, type Identity } from "./runs.ts";
import { readCandidate } from "./build.ts";
import { Owned, registerGroup } from "./owned.ts";

/** Linux Xvfb's abstract socket and lock are native resources, not portable path text. */
export function displayListening(display: string): boolean {
  const path = `@/tmp/.X11-unix/X${display.slice(1)}`;
  return readFileSync("/proc/net/unix", "utf8").split("\n").some((line) => line.trim().split(/\s+/).at(-1) === path);
}
export function displayLocked(display: string): boolean {
  return existsSync(`/tmp/.X${display.slice(1)}-lock`);
}

/** Linux's browser opener is replaced only inside the app's inherited environment. */
export function browserStandIn(fixture: Fixture, output: string): Record<string, string> {
  const bin = join(fixture.root, "bin");
  mkdirSync(bin, { recursive: true });
  const opener = join(bin, "xdg-open");
  const quoted = "'" + output.replaceAll("'", "'\\''") + "'";
  writeFileSync(opener, `#!/bin/sh\nprintf '%s\\n' "$1" >> ${quoted}\n`);
  chmodSync(opener, 0o755);
  return { PATH: [bin, process.env.PATH].filter(Boolean).join(delimiter) };
}

/** A real WebKit app launch takes about 32 seconds on CI; callers must allow it to finish. */
export const LAUNCH_TIMEOUT = 60000;

/** The suite has declared an immutable candidate; never rebuild or inspect a shared target. */
export function checkBuild() {
  readCandidate();
}

const windows = new WeakMap<object, Identity>();

/** Observe the established native app lifetime, without sending a command to a
 * dead WebView/session. Only that exact process birth ending proves closure;
 * unrelated transport failures and an unregistered browser cannot count as closed. */
export async function windowClosed(browser: object): Promise<boolean> {
  const owner = windows.get(browser);
  if (!owner) throw new Error("window lifecycle requires an owned native app session");
  return !running(owner);
}

function appOwner(group: number): Identity | null {
  const executable = app();
  for (const pid of readdirSync("/proc").filter((name) => /^\d+$/.test(name))) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/);
      if (Number(fields[2]) === group && readlinkSync(`/proc/${pid}/exe`) === executable) return identity(Number(pid));
    } catch (error) {
      if (["ENOENT", "ESRCH", "EACCES"].includes((error as NodeJS.ErrnoException).code ?? "")) continue;
      throw error;
    }
  }
  return null;
}

export type Session = {
  browser: WebdriverIO.Browser;
  /** The private X display, on Linux. */
  display: string | null;
  /** The process groups this session owns: tauri-driver's (with the app) and Xvfb's. */
  groups: number[];
  /** End the session and wait until every process it owns is gone. */
  stop: () => Promise<void>;
  /** Final case teardown: close the app, join its durable work, then remove inputs. */
  dispose: () => Promise<void>;
};

/** Ask the OS for distinct free endpoints, not PID-modulo fixed names shared by suites.
 * The drivers cannot inherit bound sockets: release immediately before launch, then
 * verify BOTH listeners belong to this process group before sending any commands. */
async function ports(): Promise<number[]> {
  const servers = [createServer(), createServer()];
  const acquired = await Promise.allSettled(servers.map((server) => new Promise<number>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") fail(new Error("no reserved driver endpoint"));
      else done(address.port);
    });
  })));
  const closed = await Promise.allSettled(servers.map((server) => new Promise<void>((done, fail) => {
    if (!server.listening) return done();
    server.close((error) => error ? fail(error) : done());
  })));
  const failures = [...acquired, ...closed].flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
  if (failures.length) throw new AggregateError(failures, "driver endpoint acquisition or release failed");
  return acquired.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
}

/** A free-port handoff can lose a race to an unrelated process. Never connect to it:
 * Linux socket inodes establish that the real listener is in our driver's group. */
function listening(port: number, group: number): boolean {
  const inodes = new Set(["/proc/net/tcp", "/proc/net/tcp6"].flatMap((path) => readFileSync(path, "utf8").trim().split("\n").slice(1)
    .map((line) => line.trim().split(/\s+/)).filter((fields) => fields[3] === "0A" && parseInt(fields[1].split(":")[1], 16) === port).map((fields) => `socket:[${fields[9]}]`)));
  if (!inodes.size) return false;
  return readdirSync("/proc").filter((name) => /^\d+$/.test(name)).some((pid) => {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      if (Number(stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[2]) !== group) return false;
      return readdirSync(`/proc/${pid}/fd`).some((fd) => inodes.has(readlinkSync(`/proc/${pid}/fd/${fd}`)));
    } catch (error) {
      if (["ENOENT", "ESRCH", "EACCES"].includes((error as NodeJS.ErrnoException).code ?? "")) return false;
      throw error;
    }
  });
}

async function until(check: () => Promise<boolean>, what: string, ms = 20000): Promise<void> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error(`timed out waiting for ${what}`);
}

/** Start the app on `fixture` with `args` (such as `review`) and connect to its webview. */
export async function start(fixture: Fixture, ...args: string[]): Promise<Session> {
  return launch(fixture, {}, args);
}

/** Start a tool session from its explicitly prepared, session-private executable snapshot. */
export async function startBuilt(fixture: Fixture, ...args: string[]): Promise<Session> {
  return launch(fixture, {}, args);
}

/**
 * Start the app with `env` over the test environment: a value replaces, `undefined` removes
 * (for example `ARTIFACTIZE_REMOTE`, which the other tests turn off).
 */
export async function startEnv(fixture: Fixture, env: Record<string, string | undefined>, ...args: string[]): Promise<Session> {
  return launch(fixture, env, args);
}

type Display = { env: Record<string, string>; server: Owned | null };

/**
 * A private X display for the app on Linux, so a run never opens windows on the person's
 * desktop or takes its focus, locally as in CI: Xvfb picks a free display number and
 * reports it (-displayfd), and the session stops it. This harness rejects other platforms.
 */
async function display(): Promise<Display> {
  if (process.platform !== "linux") return { env: {}, server: null };
  // Display numbers far from a desktop's own (:0, :1), and apart per test process.
  const base = 100 + (process.pid % 400) * 4;
  let failure = "";
  for (let number = base; number < base + 40; number += 1) {
    const started = await xvfb(number);
    if ("server" in started) return started;
    failure = started.failure;
  }
  throw new Error(`Xvfb found no free display from :${base}: ${failure}`);
}

/**
 * Xvfb on display `number`, ready once it reports the number (-displayfd). It listens on the
 * abstract socket only: no TCP, and no /tmp/.X11-unix socket, which a desktop (WSLg) may own.
 */
async function xvfb(number: number): Promise<Display | { failure: string }> {
  const owned = Owned.spawn(
    "Xvfb",
    [`:${number}`, "-displayfd", "3", "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-nolisten", "unix"],
    { stdio: ["ignore", "ignore", "pipe", "pipe"] },
  );
  const server = owned.child;
  let errors = "";
  server.stderr?.on("data", (chunk) => { errors += chunk; });
  try {
    const selected = await new Promise<string | null>((done, fail) => {
      const reported = server.stdio[3] as NodeJS.ReadableStream;
      let text = "";
      const deadline = setTimeout(() => finish(new Error("Xvfb did not report display readiness")), LAUNCH_TIMEOUT);
      const error = (error: Error) => finish(error);
      const close = () => finish(undefined, null);
      const data = (chunk: Buffer) => {
        text += chunk;
        if (text.includes("\n")) finish(undefined, text.trim());
      };
      function finish(error?: Error, value: string | null = null) {
        clearTimeout(deadline);
        reported.off("data", data);
        server.off("error", errorHandler);
        server.off("close", close);
        if (error) fail(error); else done(value);
      }
      const errorHandler = error;
      server.once("error", errorHandler);
      server.once("close", close);
      reported.on("data", data);
    });
    if (selected === null) {
      await owned.stop();
      return { failure: errors.trim() };
    }
    if (selected !== String(number)) throw new Error(`Xvfb reported an unexpected display: ${selected}`);
    return { env: { DISPLAY: `:${selected}`, GDK_BACKEND: "x11" }, server: owned };
  } catch (error) {
    try { await owned.stop(); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "Xvfb setup and shutdown failed"); }
    throw error;
  }
}

async function launch(fixture: Fixture, overrides: Record<string, string | undefined>, args: string[]): Promise<Session> {
  checkBuild();
  if (process.platform !== "linux") throw new Error("the native E2E driver requires Linux WebKitGTK, procfs and private Xvfb displays");
  const [port, nativePort] = await ports();
  const env: Record<string, string | undefined> = { ...environment(fixture), ...overrides };
  const screen = await display();
  const owned: Owned[] = screen.server ? [screen.server] : [];
  const end = async () => {
    const results = await Promise.allSettled(owned.map((child) => child.stop()));
    const errors = results.filter((result) => result.status === "rejected").map((result) => result.reason as unknown);
    if (errors.length) throw new AggregateError(errors, "native session groups did not stop");
  };
  try {
    for (const child of owned) registerGroup(fixture.root, child.group);
    if (screen.env.DISPLAY) delete env.WAYLAND_DISPLAY;
    for (const [name, value] of Object.entries(env)) if (value === undefined) delete env[name];
    // A private session bus prevents GTK from using or auto-launching the person's
    // shared bus. It, tauri-driver, WebKitWebDriver and the app share our owned group.
    const driver = Owned.spawn("dbus-run-session", ["--", "tauri-driver", "--port", String(port), "--native-port", String(nativePort)], {
      env: {
        ...env,
        ...screen.env,
        // A fresh WebView profile per run: no pane sizes or other storage from earlier runs.
        XDG_DATA_HOME: join(fixture.root, "data"),
        // Typed keys reach the app through GTK's built-in input method: literal text, never a
        // desktop input method's composition (Korean input is a manual test, see the README).
        GTK_IM_MODULE: "gtk-im-context-simple",
        XMODIFIERS: "",
      },
      stdio: ["ignore", "ignore", "inherit"],
    });
    owned.push(driver);
    await new Promise<void>((done, fail) => {
      driver.child.once("spawn", () => done());
      driver.child.once("error", fail);
    });
    registerGroup(fixture.root, driver.group);
    await until(async () => listening(port, driver.group) && listening(nativePort, driver.group), "this session's driver listeners");
    const browser = await connect(port, args, fixture);
    const owner = appOwner(driver.group);
    if (!owner || !running(owner)) throw new Error("the connected native app has no live owned process identity");
    windows.set(browser, owner);
    let ending: Promise<void> | undefined;
    // A dead WebView can leave DELETE session unanswered even when /status says
    // No sessions. Native ownership joins the driver, app and stdio directly;
    // cleanup must not queue another protocol request ahead of that release.
    const stop = () => ending ??= end();
    return {
      browser,
      display: screen.env.DISPLAY ?? null,
      groups: owned.map((child) => child.group),
      stop,
      dispose: async () => { await stop(); removeFixture(readCandidate().cli, fixture.root); },
    };
  } catch (error) {
    try { await end(); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "native launch and cleanup failed"); }
    throw error;
  }
}

async function connect(port: number, args: string[], fixture: Fixture): Promise<WebdriverIO.Browser> {
  return remote({
    hostname: "127.0.0.1",
    port,
    logLevel: "error",
    connectionRetryTimeout: LAUNCH_TIMEOUT,
    capabilities: {
      // tauri-driver turns these into WebKitWebDriver's browser options.
      "tauri:options": {
        application: app(),
        // A plain launch lists every repository in the state; tests that narrow pass `--repo`.
        args: [...args, "--state-dir", fixture.state, "--reviewer", "alice"],
      },
    } as WebdriverIO.Capabilities,
  });
}
