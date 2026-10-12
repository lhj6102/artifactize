// Drive the real app through tauri-driver and WebKitWebDriver.
import { spawnSync } from "node:child_process";
import { createConnection } from "node:net";
import { existsSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { remote } from "webdriverio";
import { app, cli, type Fixture } from "./fixture.ts";
import { Owned } from "./owned.ts";

/** The newest modification time of any file below `directory`. */
function newest(directory: string): number {
  return readdirSync(directory, { withFileTypes: true }).reduce((latest, entry) => {
    const path = join(directory, entry.name);
    return Math.max(latest, entry.isDirectory() ? newest(path) : statSync(path).mtimeMs);
  }, 0);
}

/**
 * Build the app the tests drive, then check it. `cargo test -p artifactize-desktop` replaces
 * the same binary with one built for tests (Tauri's test feature), and the app embeds ui/dist
 * when it compiles: a run on either would mislead. Building here (a no-op when it is current)
 * puts the normal app back; an app still older than ui/dist means `pnpm build` ran after
 * the build, so say so at once instead of letting tests time out.
 */
function checkBuild() {
  const root = resolve(import.meta.dirname, "../../../..");
  const built = spawnSync("cargo", ["build", "-p", "artifactize", "-p", "artifactize-desktop"], {
    cwd: root,
    stdio: ["ignore", "ignore", "inherit"],
  });
  if (built.status !== 0) throw new Error("cargo build -p artifactize -p artifactize-desktop failed; see its output above.");
  assertFresh();
}

/** The app embeds ui/dist when it compiles, so it must not be older than the files there. */
export function assertFresh() {
  const dist = resolve(import.meta.dirname, "../dist");
  if (statSync(app).mtimeMs < newest(dist)) {
    throw new Error(`${app} is older than ${dist}; run \`pnpm build\` before \`cargo build\`, not after.`);
  }
}
let checked = false;

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
    socket.once("connect", () => (socket.end(), done(true)));
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

/**
 * Start the app like `start` from binaries that are already built (and only checked): for a
 * harness that runs where cargo is unavailable.
 */
export async function startBuilt(fixture: Fixture, ...args: string[]): Promise<Session> {
  if (!existsSync(app) || !existsSync(cli)) {
    throw new Error("Build first: cargo build -p artifactize -p artifactize-desktop (after pnpm build in the ui folder).");
  }
  assertFresh();
  checked = true;
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
  if (!checked) {
    checkBuild();
    checked = true;
  }
  const port = next;
  next += 2;
  const env: Record<string, string | undefined> = {
    ...process.env,
    ARTIFACTIZE_REMOTE: "off",
    ...overrides,
  };
  const screen = await display();
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
  const owned = [driver, ...(screen.server ? [screen.server] : [])];
  const end = () => Promise.all(owned.map((child) => child.stop()));
  const browser = await connect(port, args, fixture).catch(async (error) => {
    await end();
    throw error;
  });
  return {
    browser,
    display: screen.env.DISPLAY ?? null,
    groups: owned.map((child) => child.group),
    stop: async () => {
      await browser.deleteSession().catch(() => {});
      await end();
    },
  };
}

async function connect(port: number, args: string[], fixture: Fixture): Promise<WebdriverIO.Browser> {
  await until(() => listening(port), "tauri-driver");
  return remote({
    hostname: "127.0.0.1",
    port,
    logLevel: "error",
    capabilities: {
      // tauri-driver turns these into WebKitWebDriver's browser options.
      "tauri:options": {
        application: app,
        // `--all` replaces `--repo`: the app then lists every repository in the state.
        args: [...args, ...(args.includes("--all") ? [] : ["--repo", fixture.repo]), "--state-dir", fixture.state, "--reviewer", "alice"],
      },
    } as WebdriverIO.Capabilities,
  });
}
