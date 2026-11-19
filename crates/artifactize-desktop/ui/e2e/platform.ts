// The desktop E2E platform layer (E2E-PLATFORM): the only module that names an operating
// system or branches on one, and only for what really differs between them:
// - the primary shortcut modifier and its label (⌘ on macOS, Ctrl elsewhere);
// - executable file names, '/' text paths and paths that cannot be read as URLs;
// - environment variable names, case-insensitive on Windows, and where each OS keeps a
//   user's home, data and web storage;
// - how a command is found on PATH (a .cmd shim on Windows, an executable script elsewhere),
//   how pnpm is started, and how a process tree is stopped;
// - Linux runs the runner inside its own dbus-run-session and xvfb-run, on X11;
// - a Windows desktop gives page focus events only to its one foreground window, so Windows
//   runs one app at a time and gives its window the focus; macOS keeps covered windows drawing;
// - where the app finds the system browser's opener (a command on PATH on Unix, the test
//   build's opener seam on Windows). The stand-in it runs is the same on every OS.
import type {} from "@wdio/tauri-service";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, delimiter, isAbsolute, join, relative, sep, toNamespacedPath } from "node:path";
import { callWindow, makeDirectory, nativePath, writeText, type NativePath } from "./portable.ts";

const WINDOWS = process.platform === "win32";
const MACOS = process.platform === "darwin";
const LINUX = process.platform === "linux";

/** The modifier of the app's primary shortcuts as a person on this OS presses it. */
export const primaryModifier: "Meta" | "Control" = MACOS ? "Meta" : "Control";

/** A shortcut as the app labels it on this OS. */
export function shortcutLabel(keys: string): string {
  return MACOS ? keys.replaceAll("Ctrl+", "⌘") : keys;
}

/** This Node.js executable: fixtures declare it as the program the product runs. */
export const nodeProgram = process.execPath as NativePath;

/** The file name of an executable built for this OS. */
export function executableName(name: string): string {
  return WINDOWS ? `${name}.exe` : name;
}

/** The `/`-separated text form of a native path, as the product shows and stores paths. */
export function portablePath(path: NativePath): string {
  return WINDOWS ? path.replaceAll(sep, "/") : path;
}

/** The repository-relative form of `path` as the app shows it. */
export function relativeTextPath(root: NativePath, path: NativePath): string {
  const within = relative(root, path);
  if (within === "" || within.startsWith("..") || isAbsolute(within)) throw new Error(`path ${path} is not inside ${root}`);
  return portablePath(within as NativePath);
}

/** A folder for a setting that takes a URL or a path. A Windows drive path such as C:\x also
 * parses as a URL with the scheme "c", so it is given in its namespaced form, which only a
 * path can be; other systems' absolute paths are no URL. */
export function pathNotUrl(path: NativePath): string {
  return toNamespacedPath(path);
}

// ---------------------------------------------------------------------------------
// Environment

export type Environment = NodeJS.ProcessEnv;

function sameName(left: string, right: string): boolean {
  return WINDOWS ? left.toUpperCase() === right.toUpperCase() : left === right;
}

/** A variable of this process; Windows variable names ignore case. */
export function variable(name: string): string | undefined {
  const found = Object.keys(process.env).find((key) => sameName(key, name));
  return found === undefined ? undefined : process.env[found];
}

/** Set (or with `undefined`, remove) a variable of this process. */
export function setVariable(name: string, value: string | undefined): void {
  for (const key of Object.keys(process.env)) if (sameName(key, name)) delete process.env[key];
  if (value !== undefined) process.env[name] = value;
}

/** `env` with `name` set to `value`, removing differently spelled Windows aliases. */
export function withVariable(env: Environment, name: string, value: string): Environment {
  const copy = { ...env };
  for (const key of Object.keys(copy)) if (sameName(key, name)) delete copy[key];
  copy[name] = value;
  return copy;
}

/** A copy of this process's environment for one fixture: variables `drop` selects (by
 * upper-case name) are removed, and every place an OS keeps a user's home, configuration,
 * data, cache, web storage and temporary files, and the user's name, point inside `root`. */
export function isolatedEnvironment(root: NativePath, drop: (name: string) => boolean): Environment {
  let env: Environment = {};
  for (const [name, value] of Object.entries(process.env)) {
    if (!drop(name.toUpperCase()) && !["HOMEDRIVE", "HOMEPATH", "USER", "USERNAME", "LOGNAME"].includes(name.toUpperCase())) env[name] = value;
  }
  const folders: Record<string, string> = {
    HOME: "home", USERPROFILE: "home", APPDATA: "appdata", LOCALAPPDATA: "localappdata",
    XDG_CONFIG_HOME: "config", XDG_DATA_HOME: "data", XDG_STATE_HOME: "user-state", XDG_CACHE_HOME: "cache",
    XDG_RUNTIME_DIR: "runtime", TMPDIR: "tmp", TEMP: "tmp", TMP: "tmp", GNUPGHOME: "gnupg",
    // WebView2 finds its storage through the Windows known folders, which ignore the
    // variables above; this one takes precedence over them.
    ...(WINDOWS ? { WEBVIEW2_USER_DATA_FOLDER: "webview" } : {}),
  };
  for (const [name, folder] of Object.entries(folders)) env = withVariable(env, name, makeDirectory(nativePath(root, folder), { private: true }));
  // WKWebView keeps its website data under the CoreFoundation home, which ignores HOME.
  if (MACOS) env.CFFIXED_USER_HOME = env.HOME;
  const home = nativePath(root, "home");
  const drive = WINDOWS ? home.slice(0, home.indexOf(sep)) : "";
  return { ...env, ...(WINDOWS ? { HOMEDRIVE: drive, HOMEPATH: home.slice(drive.length) } : {}), USER: "alice", USERNAME: "alice", LOGNAME: "alice" };
}

function programPath(env: Environment): string[] {
  const key = Object.keys(env).find((name) => sameName(name, "PATH"));
  return (key === undefined ? "" : env[key] ?? "").split(delimiter).filter(Boolean);
}

/** `env` with PATH starting with `folders`, then those of its PATH that `keep` accepts. */
export function withPathFirst(env: Environment, folders: string[], keep: (folder: string) => boolean = () => true): Environment {
  return withVariable(env, "PATH", [...folders, ...programPath(env).filter(keep)].join(delimiter));
}

// ---------------------------------------------------------------------------------
// Commands and processes

/** Write the command `name` into `bin` as this OS finds commands on PATH (a `.cmd` shim on
 * Windows, an executable script elsewhere): it runs DESKTOP_TEST_NODE with the script the
 * variable `scriptVariable` names, passing its arguments on. Returns the command's file name. */
export function writeNodeCommand(bin: NativePath, name: string, scriptVariable: string): string {
  if (WINDOWS) {
    writeText(nativePath(bin, `${name}.cmd`), `@"%DESKTOP_TEST_NODE%" "%${scriptVariable}%" %*\r\n`);
    return `${name}.cmd`;
  }
  writeFileSync(join(bin, name), `#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$${scriptVariable}" "$@"\n`, { mode: 0o755 });
  return name;
}

/** Whether `folder` holds a program a shell would run as `name`. */
export function folderHoldsCommand(folder: string, name: string): boolean {
  try {
    if (WINDOWS) {
      const extensions = (variable("PATHEXT") ?? ".COM;.EXE;.BAT;.CMD").split(";").filter(Boolean).map((ext) => ext.toLowerCase());
      return readdirSync(folder).some((entry) => extensions.some((ext) => entry.toLowerCase() === `${name}${ext}`));
    }
    const path = join(folder, name);
    return existsSync(path) && (statSync(path).mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

/** pnpm as this run can start it: the entry point pnpm passed to this script, else (a Human
 * tool started by node) pnpm on PATH, through its JS entry on Windows, whose .cmd shim Node
 * cannot spawn without a shell. */
export function packageCommand(args: string[]): { program: string; args: string[] } {
  const entry = variable("npm_execpath");
  if (entry && isAbsolute(entry)) {
    if (/^pnpm\.(?:c|m)?js$/i.test(basename(entry))) return { program: process.execPath, args: [entry, ...args] };
    return { program: entry, args };
  }
  if (WINDOWS) {
    for (const folder of programPath(process.env)) {
      if (existsSync(join(folder, "pnpm.exe"))) return { program: join(folder, "pnpm.exe"), args };
      const script = join(folder, "node_modules", "pnpm", "bin", "pnpm.cjs");
      if (existsSync(script)) return { program: process.execPath, args: [script, ...args] };
    }
    throw new Error("pnpm is missing: install pnpm, or start through pnpm");
  }
  return { program: "pnpm", args };
}

/** Start a program as the root of a process tree that `stopTree` can end whole: a process
 * group of its own on Unix; Windows ends a tree by its root's ID. */
export function startTree(program: string, args: readonly string[], options: { cwd: NativePath; env: Environment; capture: boolean }): ChildProcess {
  return spawn(program, args, {
    cwd: options.cwd, env: options.env, detached: !WINDOWS, windowsHide: true,
    stdio: ["ignore", options.capture ? "pipe" : "inherit", "inherit"],
  });
}

/** End the process tree `startTree` started, its root and every descendant. */
export function stopTree(child: ChildProcess): void {
  if (child.pid === undefined || child.exitCode !== null) return;
  if (WINDOWS) spawnSync("taskkill", ["/T", "/F", "/PID", String(child.pid)], { windowsHide: true });
  else {
    try { process.kill(-child.pid, "SIGKILL"); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
  }
}

// ---------------------------------------------------------------------------------
// The app's session

/** The runner command `args` (a Node script and its arguments) as this OS runs it headless:
 * on Linux inside its own D-Bus session and X server, so no app reaches the user's desktop. */
export function runnerCommand(args: string[], env: Environment = process.env): { program: string; args: string[]; env: Environment } {
  if (!LINUX) return { program: process.execPath, args, env };
  // The batch checks that it got a display and a bus of its own, not these (assertHeadless).
  const outer = { DESKTOP_TEST_OUTER_DISPLAY: env.DISPLAY ?? "", DESKTOP_TEST_OUTER_BUS: env.DBUS_SESSION_BUS_ADDRESS ?? "" };
  const headless: Environment = { ...env, ...outer, GDK_BACKEND: "x11" };
  delete headless.WAYLAND_DISPLAY;
  return { program: "dbus-run-session", args: ["--", "xvfb-run", "-a", process.execPath, ...args], env: headless };
}

/** Fail unless this process runs where `runnerCommand` puts it: on Linux, a display and a
 * session bus of its own, never the ones it was started from nor a Wayland desktop. */
export function assertHeadless(): void {
  const { DISPLAY: display, DBUS_SESSION_BUS_ADDRESS: bus, DESKTOP_TEST_OUTER_DISPLAY: outerDisplay, DESKTOP_TEST_OUTER_BUS: outerBus } = process.env;
  if (LINUX && (!display || !bus || outerDisplay === undefined || outerBus === undefined || display === outerDisplay || bus === outerBus || process.env.WAYLAND_DISPLAY)) {
    throw new Error("Linux desktop E2E runs inside dbus-run-session -- xvfb-run -a: start it with `pnpm e2e`");
  }
}

/** The display and session variables an app needs on this OS, kept through fixture isolation;
 * on Linux GTK's input method is fixed for the run instead of the host's. */
export function displayEnvironment(): Environment {
  if (!LINUX) return {};
  const env: Environment = { GDK_BACKEND: "x11", GTK_IM_MODULE: "gtk-im-context-simple", XMODIFIERS: "", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" };
  for (const name of ["DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS"]) if (process.env[name] !== undefined) env[name] = process.env[name];
  return env;
}

/** The test app's window options: macOS would stop drawing a window another app covers. */
export function testWindowOptions(windows: readonly Record<string, unknown>[]): Record<string, unknown> {
  return MACOS ? { windows: windows.map((window) => ({ ...window, backgroundThrottling: "disabled" })) } : {};
}

/** How many test apps run at once. WebView2 gives focus events only to the page of the
 * desktop's one foreground window, so Windows runs one app at a time. */
export const appsAtOnce = WINDOWS ? 1 : 2;

/** On Windows, give the scenario's window the desktop's focus before it starts (see
 * appsAtOnce); other systems deliver focus events to a page in any window. */
export async function takeSystemFocus(browser: WebdriverIO.Browser): Promise<void> {
  if (!WINDOWS) return;
  await callWindow(browser, "setFocus");
  await browser.waitUntil(() => browser.execute(() => document.hasFocus()),
    { timeout: 15000, timeoutMsg: "setup failed: the app window did not take the system focus" });
}

/** Where the app finds the system browser's opener, made to run `client` (a Node program given
 * the address) instead of the person's browser: on Unix the opener command first on its PATH,
 * on Windows the test build's opener seam, which stands where the default association would. */
export function installBrowserOpener(root: NativePath, env: Environment, client: NativePath): Environment {
  const browser = { DESKTOP_TEST_NODE: nodeProgram, DESKTOP_TEST_BROWSER_CLIENT: client };
  if (WINDOWS) return { ...env, ...browser, ARTIFACTIZE_E2E_BROWSER_NODE: nodeProgram, ARTIFACTIZE_E2E_BROWSER_PROGRAM: client };
  const bin = makeDirectory(nativePath(root, "browser-bin"));
  writeNodeCommand(bin, MACOS ? "open" : "xdg-open", "DESKTOP_TEST_BROWSER_CLIENT");
  return { ...withPathFirst(env, [bin]), ...browser };
}
