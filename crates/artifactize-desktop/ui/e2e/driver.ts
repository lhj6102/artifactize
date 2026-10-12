// Drive the real app through tauri-driver and WebKitWebDriver.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { createConnection } from "node:net";
import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { remote } from "webdriverio";
import { app, type Fixture } from "./fixture.ts";

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
  const dist = resolve(import.meta.dirname, "../dist");
  if (statSync(app).mtimeMs < newest(dist)) {
    throw new Error(`${app} is older than ${dist}; run \`pnpm build\` before \`cargo build\`, not after.`);
  }
}
let checked = false;

export type Session = { browser: WebdriverIO.Browser; stop: () => Promise<void> };

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

/**
 * Typed keys reach the app through GTK's input method. The tests type literal text with GTK's
 * built-in method; `desktop` keeps the desktop's own (for example fcitx5 Hangul) for the IME
 * test, which then composes what WebDriver types.
 */
export type Ime = "literal" | "desktop";

/** Start the app on `fixture` with `args` (such as `review`) and connect to its webview. */
export async function start(fixture: Fixture, ...args: string[]): Promise<Session> {
  return startWith("literal", fixture, ...args);
}

export async function startWith(ime: Ime, fixture: Fixture, ...args: string[]): Promise<Session> {
  return launch(ime, fixture, {}, args);
}

/**
 * Start the app with `env` over the test environment: a value replaces, `undefined` removes
 * (for example `ARTIFACTIZE_REMOTE`, which the other tests turn off).
 */
export async function startEnv(fixture: Fixture, env: Record<string, string | undefined>, ...args: string[]): Promise<Session> {
  return launch("literal", fixture, env, args);
}

async function launch(ime: Ime, fixture: Fixture, overrides: Record<string, string | undefined>, args: string[]): Promise<Session> {
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
  for (const [name, value] of Object.entries(env)) if (value === undefined) delete env[name];
  const driver: ChildProcess = spawn("tauri-driver", ["--port", String(port), "--native-port", String(port + 1)], {
    env: {
      ...env,
      // A fresh WebView profile per run: no pane sizes or other storage from earlier runs.
      XDG_DATA_HOME: join(fixture.root, "data"),
      ...(ime === "literal" ? { GTK_IM_MODULE: "gtk-im-context-simple", XMODIFIERS: "" } : {}),
    },
    stdio: ["ignore", "ignore", "inherit"],
  });
  await until(() => listening(port), "tauri-driver");
  const browser = await remote({
    hostname: "127.0.0.1",
    port,
    logLevel: "error",
    capabilities: {
      // tauri-driver turns these into WebKitWebDriver's browser options.
      "tauri:options": {
        application: app,
        args: [...args, "--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"],
      },
    } as WebdriverIO.Capabilities,
  });
  return {
    browser,
    stop: async () => {
      await browser.deleteSession().catch(() => {});
      driver.kill();
    },
  };
}
