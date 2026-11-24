// Shared owners for Linux media capture. The service starts and stops the desktop. Only a
// failed service stop uses the fixture app's birth receipt; no other process can be targeted.
import { spawn, type ChildProcess } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { stopTree, type Environment } from "../../crates/artifactize-desktop/ui/e2e/platform.ts";
import { embeddedPorts, nativePath, runProgramOk } from "../../crates/artifactize-desktop/ui/e2e/portable.ts";
import type { Owned } from "../../crates/artifactize-desktop/ui/e2e/fixtures/owned.ts";

const service = createRequire(new URL("../../crates/artifactize-desktop/ui/package.json", import.meta.url))("@wdio/tauri-service") as
  typeof import("../../crates/artifactize-desktop/ui/node_modules/@wdio/tauri-service");
const STOP_TIMEOUT = 15000;
const WRAPPER_TIMEOUT = 90000; // Allow the bounded active WebDriver command and teardown to finish.

async function bounded<T>(work: Promise<T>, timeout: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([work, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} did not stop`)), timeout);
    })]);
  } finally { clearTimeout(timer); }
}

/** Start the display and private bus as one tree. EOF reaches the recording even if the
 * signal was sent only to the outer command. Do not kill Xvfb before the app has stopped. */
export async function onDisplay(script: URL, mode: string[], width: number, height: number, quiet: boolean): Promise<number> {
  const env: Environment = { ...process.env, CAPTURE_DISPLAY_OWNED: "1", GDK_BACKEND: "x11", GDK_SCALE: "1", GDK_DPI_SCALE: "1", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" };
  delete env.WAYLAND_DISPLAY;
  const child = spawn("dbus-run-session", ["--", "xvfb-run", "-a", "-s", `-screen 0 ${width}x${height}x24`, process.execPath, fileURLToPath(script), ...mode],
    { env, detached: true, stdio: ["pipe", quiet ? process.stderr : "inherit", "inherit"] });
  const closed = new Promise<number>((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code) => resolve(code ?? 1));
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const requestStop = () => {
    if (timer) return;
    child.stdin.end();
    timer = setTimeout(() => stopTree(child), WRAPPER_TIMEOUT);
  };
  child.stdin.on("error", () => { /* an exited wrapper has already closed the pipe */ });
  process.on("SIGINT", requestStop);
  process.on("SIGTERM", requestStop);
  try { return await closed; }
  finally {
    process.off("SIGINT", requestStop);
    process.off("SIGTERM", requestStop);
    clearTimeout(timer);
  }
}

export const captureAbort = new AbortController();
export function watchCaptureStop(): () => void {
  const stop = () => captureAbort.abort(new Error("Media capture interrupted"));
  process.stdin.resume();
  // A build step also owns a detached Cargo process group and handles SIGTERM itself.
  const lostParent = () => { stop(); process.emit("SIGTERM", "SIGTERM"); };
  process.stdin.once("end", lostParent);
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  return () => {
    process.stdin.off("end", lostParent);
    process.stdin.pause();
    process.off("SIGINT", stop);
    process.off("SIGTERM", stop);
  };
}

export async function launchCapture(scope: Owned, app: string, args: string[], env: Environment, width: number, height: number, wait: number, register: (stop: () => Promise<void>) => void) {
  const receipt = nativePath(scope.root, "app-process.json");
  const appEnv = { ...env, ARTIFACTIZE_E2E_OWNER_FILE: receipt };
  const saved = { ...process.env };
  const { ports: [embeddedPort], close } = await embeddedPorts(1);
  let browser: WebdriverIO.Browser | undefined;
  let stopping: Promise<void> | undefined;
  const stop = () => stopping ??= (async () => {
    let serviceFailure: unknown;
    try { if (browser) await bounded(service.cleanupWdioSession(browser), STOP_TIMEOUT, "the media WebDriver service"); }
    catch (error) { serviceFailure = error; }
    try {
      // This also proves native exit after the service returns, before inputs are touched.
      runProgramOk(app, ["--stop-e2e-owner", receipt], { cwd: scope.root, env: appEnv, timeout: STOP_TIMEOUT });
    } finally { await close(); }
    await scope.close();
    if (serviceFailure) throw serviceFailure;
  })();
  register(stop);
  try {
    for (const name of Object.keys(process.env)) if (appEnv[name as keyof typeof appEnv] === undefined) delete process.env[name];
    Object.assign(process.env, appEnv);
    const capabilities = service.createTauriCapabilities(app, { appArgs: args, driverProvider: "embedded", startTimeout: wait, commandTimeout: wait, logLevel: "error" });
    capabilities["wdio:enforceWebDriverClassic"] = true;
    capabilities["wdio:tauriServiceOptions"] = { ...capabilities["wdio:tauriServiceOptions"], embeddedPort };
    browser = await service.startWdioSession(capabilities);
    await browser.setWindowSize(width, height);
    await browser.waitUntil(() => browser!.execute(() => [innerWidth, innerHeight].join("x")).then((size) => size === `${width}x${height}`),
      { timeout: wait, timeoutMsg: `the media fixture window is ${width}x${height}` });
    captureAbort.signal.throwIfAborted();
    return { browser, stop };
  } catch (error) {
    try { await stop(); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "media startup and teardown failed"); }
    throw error;
  } finally {
    for (const name of Object.keys(process.env)) if (!(name in saved)) delete process.env[name];
    Object.assign(process.env, saved);
  }
}

/** Watchers do not own durable work; stop the watcher, then release the fixture separately. */
export async function endWatcher(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = new Promise<void>((resolve) => child.once("close", () => resolve()));
  child.kill("SIGKILL");
  await bounded(closed, STOP_TIMEOUT, "the media Run watcher");
}
