// Drive the real Linux WebKitGTK app through tauri-driver and WebKitWebDriver.
// These shared native helpers intentionally own Linux X11/process-group setup: Windows
// uses a different WebDriver and macOS has no Tauri WebDriver, neither is this harness.
import { createConnection } from "node:net";
import { delimiter, join } from "node:path";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { remote } from "webdriverio";
import { app, type Fixture } from "./fixture.ts";
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

export type Session = {
  browser: WebdriverIO.Browser;
  /** The private X display, on Linux. */
  display: string | null;
  /** The process groups this session owns: tauri-driver's (with the app) and Xvfb's. */
  groups: number[];
  /** End the session and wait until every process it owns is gone. */
  stop: () => Promise<void>;
};

// Ports per process, so a run never meets a leftover driver of another.
let next = 4600 + (process.pid % 200) * 2;

function listening(port: number): Promise<boolean> {
  return new Promise((done) => {
    const socket = createConnection({ port, host: "127.0.0.1" });
    socket.once("connect", () => { socket.destroy(); done(true); });
    socket.once("error", () => done(false));
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
 * reports it (-displayfd), and the session stops it. Elsewhere the app uses the display it
 * inherits.
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
function xvfb(number: number): Promise<Display | { failure: string }> {
  const owned = Owned.spawn(
    "Xvfb",
    [`:${number}`, "-displayfd", "3", "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-nolisten", "unix"],
    { stdio: ["ignore", "ignore", "pipe", "pipe"] },
  );
  const server = owned.child;
  return new Promise((done, fail) => {
    let errors = "";
    server.stderr?.on("data", (chunk) => (errors += chunk));
    server.once("error", (error: NodeJS.ErrnoException) => {
      void owned.stop();
      fail(
        new Error(
          error.code === "ENOENT"
            ? "The e2e tests run the app on a private Xvfb display; install Xvfb (for example `apt install xvfb`)."
            : `Xvfb did not start: ${error.message}`,
        ),
      );
    });
    // In use (or refused): the caller tries the next number.
    server.once("exit", () => void owned.stop().then(() => done({ failure: errors.trim() })));
    const reported = server.stdio[3] as NodeJS.ReadableStream;
    let text = "";
    reported.on("data", (chunk) => {
      text += chunk;
      if (!text.includes("\n")) return;
      server.removeAllListeners("exit");
      // The app draws on X only: no Wayland compositor of the desktop either.
      done({ env: { DISPLAY: `:${text.trim()}`, GDK_BACKEND: "x11" }, server: owned });
    });
  });
}

async function launch(fixture: Fixture, overrides: Record<string, string | undefined>, args: string[]): Promise<Session> {
  checkBuild();
  const port = next;
  next += 2;
  const env: Record<string, string | undefined> = {
    ...process.env,
    ARTIFACTIZE_REMOTE: "off",
    ...overrides,
  };
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
    // tauri-driver starts WebKitWebDriver, which starts the app: all in the driver's group.
    const driver = Owned.spawn("tauri-driver", ["--port", String(port), "--native-port", String(port + 1)], {
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
    const browser = await connect(port, args, fixture);
    return {
      browser,
      display: screen.env.DISPLAY ?? null,
      groups: owned.map((child) => child.group),
      stop: async () => {
        await browser.deleteSession().catch(() => {});
        await end();
      },
    };
  } catch (error) {
    try { await end(); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "native launch and cleanup failed"); }
    throw error;
  }
}

async function connect(port: number, args: string[], fixture: Fixture): Promise<WebdriverIO.Browser> {
  await until(() => listening(port), "tauri-driver");
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
