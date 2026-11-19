// The desktop E2E platform layer: the only module that names an operating system,
// reads or changes the process environment, builds native paths, touches the file
// system, starts or stops processes, or opens sockets. Every other E2E module calls
// these portable functions, so an OS difference in CI is investigated here first.
// Runner configuration is the only other place where platforms differ; specs contain
// no OS branches. The app/driver lifetime still belongs to WDIO's Tauri service.
import type {} from "@wdio/tauri-service";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { randomInt } from "node:crypto";
import {
  appendFileSync, copyFileSync, existsSync, linkSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, readlinkSync,
  realpathSync, renameSync, rmSync, writeFileSync,
} from "node:fs";
import { createServer as createHttpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createConnection, createServer, type Socket } from "node:net";
import { constants, homedir, tmpdir } from "node:os";
import { basename, delimiter, dirname, isAbsolute, join, normalize, relative, resolve, sep, toNamespacedPath } from "node:path";
import { DatabaseSync } from "node:sqlite";

export type { Socket as LoopbackSocket } from "node:net";

// ---------------------------------------------------------------------------------
// Operating system values

const WINDOWS = process.platform === "win32";
const MACOS = process.platform === "darwin";
const LINUX = process.platform === "linux";

/** The suffix of an executable file name on this OS. */
const executableSuffix = WINDOWS ? ".exe" : "";

/** The modifier of the app's primary shortcuts as a person on this OS presses it. */
export const primaryModifier: "Meta" | "Control" = MACOS ? "Meta" : "Control";

/** The platform's visible primary-shortcut label, independent of the app renderer. */
export function shortcutLabel(keys: string): string {
  return MACOS ? keys.replaceAll("Ctrl+", "⌘") : keys;
}

/** This Node.js executable: fixtures declare it as the program the product runs. */
export const nodeProgram = process.execPath as NativePath;

/** The Git program, found through the OS's program search. */
export const gitProgram = "git";

// ---------------------------------------------------------------------------------
// Paths

declare const native: unique symbol;
/** A path in this OS's own form, for file system and process calls only. */
export type NativePath = string & { readonly [native]: true };

/** Join path segments into a native path. */
export function nativePath(first: string, ...rest: string[]): NativePath {
  return join(first, ...rest) as NativePath;
}

/** An absolute native path from segments, resolved against the current directory. */
export function absolutePath(first: string, ...rest: string[]): NativePath {
  return resolve(first, ...rest) as NativePath;
}

/** Accept an absolute path from a variable, an argument or serialized data, in its native form. */
export function asNativePath(path: string): NativePath {
  if (!isAbsolute(path)) throw new Error(`not an absolute native path: ${path}`);
  return normalize(path) as NativePath;
}

export function parentOf(path: NativePath): NativePath {
  return dirname(path) as NativePath;
}

/** The repository-relative form of a path as shown by the app. */
export function relativeTextPath(root: NativePath, path: NativePath): string {
  if (!isInside(root, path)) throw new Error(`path ${path} is not inside ${root}`);
  return portablePath(relative(root, path) as NativePath);
}

export function nameOf(path: NativePath): string {
  return basename(path);
}

/** Whether `path` lies strictly inside `root`. */
export function isInside(root: NativePath, path: NativePath): boolean {
  const within = relative(root, path);
  return within !== "" && within !== ".." && !within.startsWith(`..${sep}`) && !isAbsolute(within);
}

/** A path written into a file, a declaration or an argument template, or compared with
 * what the product shows: segments joined by `/` on every OS. */
export function textPath(...segments: string[]): string {
  return segments.join("/");
}

/** The `/`-separated text form of a native path, as the product shows and stores paths. */
export function portablePath(path: NativePath): string {
  return WINDOWS ? path.replaceAll(sep, "/") : path;
}

/** The OS's canonical spelling of an existing path: links resolved, and Windows short
 * (8.3) names and macOS `/var` aliases expanded, as the product canonicalizes paths. */
export function canonicalPath(path: NativePath): NativePath {
  return realpathSync.native(path) as NativePath;
}

/** A folder for a tool setting that takes a URL or a path. A Windows drive path such as
 * C:\x also parses as a URL with the scheme "c", so it is given in its namespaced form
 * \\?\C:\x, which only a path can be; other systems' absolute paths are no URL. */
export function pathNotUrl(path: NativePath): string {
  return toNamespacedPath(path);
}

/** A file of the E2E suite itself. */
export function suitePath(...segments: string[]): NativePath {
  return join(import.meta.dirname, ...segments) as NativePath;
}

// ---------------------------------------------------------------------------------
// Files and directories

export function exists(path: NativePath): boolean {
  return existsSync(path);
}

/** Create a directory and its parents; `private` keeps it to the current user. */
export function makeDirectory(path: NativePath, { private: owner = false } = {}): NativePath {
  mkdirSync(path, { recursive: true, ...(owner ? { mode: 0o700 } : {}) });
  return path;
}

/** A new, uniquely named, canonical directory under `parent`. */
export function newDirectory(parent: NativePath, prefix: string): NativePath {
  makeDirectory(parent);
  return canonicalPath(mkdtempSync(join(parent, prefix)) as NativePath);
}

/** The directory the suite's own scratch lives under: the product's declared scratch
 * directory when the suite runs as an eval, otherwise the OS temporary directory. */
export function scratchDirectory(): NativePath {
  return (variable("ARTIFACTIZE_TMP_DIR") ?? tmpdir()) as NativePath;
}

/** Entries of `directory` that are real directories (never links), sorted by name. */
export function childDirectories(directory: NativePath): NativePath[] {
  return readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isDirectory())
    .map((entry) => join(directory, entry.name) as NativePath).sort();
}

/** Entries of `directory` that are regular files, sorted by name. */
export function listFiles(directory: NativePath): NativePath[] {
  return readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isFile())
    .map((entry) => join(directory, entry.name) as NativePath).sort();
}

export function isLink(path: NativePath): boolean {
  return lstatSync(path).isSymbolicLink();
}

/** Text files are UTF-8 and keep `\n` line endings on every OS. */
export function readText(path: NativePath): string {
  return readFileSync(path, "utf8");
}

/** Read a text file, or `null` when it does not exist. */
export function readTextIfExists(path: NativePath): string | null {
  try { return readText(path); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

/** Write a text file. `exclusive` fails when it exists; `private` keeps it to the user. */
export function writeText(path: NativePath, text: string, { exclusive = false, private: owner = false } = {}): void {
  writeFileSync(path, text, { ...(exclusive ? { flag: "wx" } : {}), ...(owner ? { mode: 0o600 } : {}) });
}

/** Write a text file only when it does not exist yet; returns whether it was written. */
export function writeTextOnce(path: NativePath, text: string, options: { private?: boolean } = {}): boolean {
  try { writeText(path, text, { ...options, exclusive: true }); return true; }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "EEXIST") return false;
    throw error;
  }
}

/** Write JSON the suite reads back: every absolute path in it is stored in '/' form. */
export function writeJson(path: NativePath, value: unknown, options: { private?: boolean } = {}): void {
  writeText(path, JSON.stringify(value, (_key, item: unknown) =>
    typeof item === "string" && isAbsolute(item) ? portablePath(item as NativePath) : item), options);
}

/** Read JSON written by `writeJson`, with its absolute paths in native form again. */
export function readJson(path: NativePath): unknown {
  return JSON.parse(readText(path), (_key, item: unknown) =>
    typeof item === "string" && isAbsolute(item) ? normalize(item) : item);
}

export function appendText(path: NativePath, text: string): void {
  appendFileSync(path, text);
}

/** Replace a file's whole content at once: readers see the old or the new file only. */
export function replaceText(path: NativePath, text: string): void {
  const pending = `${path}.${process.pid}.pending` as NativePath;
  writeText(pending, text);
  renameSync(pending, path);
}

/** Atomically close an admission directory. It must not be recreated by late clients. */
export function closeDirectory(from: NativePath, to: NativePath): void {
  if (exists(from)) renameSync(from, to);
}

export function copyFile(from: NativePath, to: NativePath): void {
  copyFileSync(from, to);
}

/** The app as a hard link in `directory`, so it runs from a place an install channel puts it
 * while the build stays one file. Removing the fixture removes only the link. */
export function placeApp(app: NativePath, directory: NativePath): NativePath {
  const placed = nativePath(makeDirectory(directory), nameOf(app));
  linkSync(app, placed);
  return placed;
}

/** A folder below `root` shaped like the app's own in a bundle dragged from the macOS disk image
 * to an Applications folder other than the person's. */
export function diskImageAppDirectory(root: NativePath): NativePath {
  return nativePath(root, "disk-image", "Applications", "Artifactize.app", "Contents", "MacOS");
}

/** A folder below `root` shaped like the one Windows installs the Microsoft Store package in. */
export function storePackageDirectory(root: NativePath): NativePath {
  return nativePath(root, "WindowsApps", "lhj6102.Artifactize_9.9.9.0_x64__ayfzfzbsv48sg");
}

/** Processes other than this one that hold a file open, or work, inside `root`. Unix lets a
 * held file be deleted, so deletion succeeding there proves nothing: Linux reads /proc and
 * macOS asks lsof. Windows refuses to delete a held file, which removeTree reports. */
function holdersOf(root: NativePath): number[] {
  if (WINDOWS) {
    // Windows refuses to rename a folder while a process holds a file inside it; the rename
    // there and back changes nothing and deletes nothing. -1: some process, unnamed.
    const probe = `${root}.release-probe`;
    try { renameSync(root, probe); }
    catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "EBUSY" || code === "EPERM" || code === "EACCES") return [-1];
      throw error;
    }
    renameSync(probe, root);
    return [];
  }
  if (MACOS) {
    const listed = spawnSync("lsof", ["-t", "+D", root], { encoding: "utf8" });
    return listed.stdout.split("\n").map(Number).filter((pid) => Number.isSafeInteger(pid) && pid > 0 && pid !== process.pid);
  }
  const inside = (target: string) => target === root || target.startsWith(`${root}${sep}`);
  const holders: number[] = [];
  for (const entry of readdirSync("/proc")) {
    const pid = Number(entry);
    if (!Number.isSafeInteger(pid) || pid === process.pid) continue;
    try {
      const fds = `/proc/${pid}/fd`;
      const held = inside(readlinkSync(`/proc/${pid}/cwd`)) || readdirSync(fds).some((fd) => {
        try { return inside(readlinkSync(join(fds, fd))); } catch { return false; }
      });
      if (held) holders.push(pid);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== "ENOENT" && code !== "EACCES" && code !== "ESRCH") throw error;
    }
  }
  return holders;
}

/** Wait, bounded, until no other process holds anything inside a fixture root: the app's web
 * engine helpers (WebKitGTK's web and network processes, WKWebView's) outlive the app for a
 * moment and keep or still write its data folders (E2E-OWNED-FIXTURES). */
export async function awaitReleased(root: NativePath, timeout = 60000): Promise<void> {
  const until = performance.now() + timeout;
  let holders = holdersOf(root);
  while (holders.length > 0) {
    if (performance.now() > until) throw new Error(`processes ${holders.join(", ")} still hold fixture ${root}; preserve inputs`);
    await new Promise((done) => setTimeout(done, 100));
    holders = holdersOf(root);
  }
}

/** Remove a directory tree. An app's web engine helper processes (WebView2, WebKitGTK's
 * web and network processes) exit after the app, and until then they keep or still write
 * the data folders that live in its fixture. Deletion retries with linearly growing pauses
 * (up to a minute in all) while they finish; a tree still busy after that is reported. */
export function removeTree(path: NativePath): void {
  rmSync(path, { recursive: true, force: true, maxRetries: 30, retryDelay: 125 });
}

// ---------------------------------------------------------------------------------
// Environment

export type Environment = NodeJS.ProcessEnv;

/** Read a variable of this process; Windows variable names ignore case. */
export function variable(name: string): string | undefined {
  if (!WINDOWS) return process.env[name];
  const found = Object.keys(process.env).find((key) => key.toUpperCase() === name.toUpperCase());
  return found === undefined ? undefined : process.env[found];
}

export function requiredVariable(name: string, why: string): string {
  const value = variable(name);
  if (!value) throw new Error(`${name} is required: ${why}`);
  return value;
}

/** Set (or with `undefined`, remove) a variable of this process. */
export function setVariable(name: string, value: string | undefined): void {
  for (const key of Object.keys(process.env)) if (sameVariable(key, name)) delete process.env[key];
  if (value !== undefined) process.env[name] = value;
}

/** Make this process's environment exactly `environment`. */
export function replaceEnvironment(environment: Environment): void {
  for (const name of Object.keys(process.env)) if (!(name in environment)) delete process.env[name];
  Object.assign(process.env, environment);
}

/** A copy of this process's environment. */
export function inheritedEnvironment(): Environment {
  return { ...process.env };
}

function sameVariable(left: string, right: string): boolean {
  return WINDOWS ? left.toUpperCase() === right.toUpperCase() : left === right;
}

/** Set `name` in a copy-owned environment, removing differently spelled Windows aliases. */
export function withVariable(environment: Environment, name: string, value: string): Environment {
  for (const key of Object.keys(environment)) if (sameVariable(key, name)) delete environment[key];
  environment[name] = value;
  return environment;
}

/** A copy of this process's environment for one fixture: variables `drop` selects (by
 * upper-case name) are removed, and every OS location of the user's home, configuration,
 * data, cache, runtime and temporary files, and the user's name, point inside `root`. */
export function isolatedEnvironment(root: NativePath, drop: (name: string) => boolean): Environment {
  const environment: Environment = {};
  for (const [name, value] of Object.entries(process.env)) if (!drop(name.toUpperCase())) environment[name] = value;
  const directories: Record<string, string> = {
    HOME: "home", USERPROFILE: "home", APPDATA: "appdata", LOCALAPPDATA: "localappdata",
    XDG_CONFIG_HOME: "config", XDG_DATA_HOME: "data", XDG_STATE_HOME: "user-state", XDG_CACHE_HOME: "cache",
    XDG_RUNTIME_DIR: "runtime", TMPDIR: "tmp", TEMP: "tmp", TMP: "tmp", GNUPGHOME: "gnupg",
  };
  for (const [name, directory] of Object.entries(directories)) {
    // Remove case-insensitive aliases too: Windows variable names ignore case.
    for (const key of Object.keys(environment)) if (key.toUpperCase() === name) delete environment[key];
    environment[name] = makeDirectory(nativePath(root, directory), { private: true });
  }
  // WebView2 finds its default storage through the Windows known folders, which ignore the
  // variables above; this variable takes precedence over the folder the app passes, so the
  // web storage of each fixture, kept across its own relaunch, is private to it.
  if (WINDOWS) {
    for (const key of Object.keys(environment)) if (key.toUpperCase() === "WEBVIEW2_USER_DATA_FOLDER") delete environment[key];
    environment.WEBVIEW2_USER_DATA_FOLDER = makeDirectory(nativePath(root, "webview"), { private: true });
  }
  // WKWebView keeps its default website data under the Library folder of the CoreFoundation
  // home, which ignores HOME but takes this variable, so that data is private to the fixture
  // too and is removed with it.
  if (MACOS) environment.CFFIXED_USER_HOME = environment.HOME;
  for (const key of Object.keys(environment)) {
    if (["HOMEDRIVE", "HOMEPATH", "USER", "USERNAME", "LOGNAME"].includes(key.toUpperCase())) delete environment[key];
  }
  const home = nativePath(root, "home");
  const drive = WINDOWS ? home.slice(0, home.indexOf(sep)) : "";
  return { ...environment, HOMEDRIVE: drive, HOMEPATH: home.slice(drive.length), USER: "alice", USERNAME: "alice", LOGNAME: "alice" };
}

// ---------------------------------------------------------------------------------
// Programs and processes

/** The file name of an executable built for this OS. */
export function executableName(name: string): string {
  return `${name}${executableSuffix}`;
}

export type Completed = { status: number; stdout: string; stderr: string };
type RunOptions = { env?: Environment; cwd?: NativePath; input?: string; timeout?: number };

/** Run a program with an argument vector (never a shell string) until it exits.
 * A program that cannot start is an error; its exit status is the caller's to judge. */
export function runProgram(program: string, args: readonly string[], options: RunOptions = {}): Completed {
  const result = spawnSync(program, args, { encoding: "utf8", ...options });
  if (result.error) throw result.error;
  return { status: result.status ?? -1, stdout: result.stdout, stderr: result.stderr };
}

/** As `runProgram`, but an exit status other than 0 is an error. */
export function runProgramOk(program: string, args: readonly string[], options: RunOptions = {}): string {
  const result = runProgram(program, args, options);
  if (result.status !== 0) throw new Error(`${program} ${args.join(" ")} exited ${result.status}: ${result.stderr}`);
  return result.stdout;
}

/** As `runProgram` without blocking this process, so a server it serves can answer. */
export async function runProgramAsync(program: string, args: readonly string[], options: RunOptions = {}): Promise<Completed> {
  const result = await fixtureRunner({ program, args: [...args], env: options.env ?? inheritedEnvironment(), cwd: options.cwd ?? asNativePath(process.cwd()) },
    AbortSignal.timeout(options.timeout ?? 60000));
  return { status: result.code ?? -1, stdout: result.stdout, stderr: result.stderr };
}

/** The e2e app can be covered by another app window during a parallel run. macOS
 * otherwise suspends WebView frames in that situation. Omit the override elsewhere. */
export function testWindowOptions(windows: readonly Record<string, unknown>[]): Record<string, unknown> {
  return MACOS ? { windows: windows.map((window) => ({ ...window, backgroundThrottling: "disabled" })) } : {};
}

/** Keep the build owner alive on interruption so it can join the compiler tree
 * before removing inputs. One signal covers both frontend and native build steps. */
export async function withBuildInterrupts<T>(build: (signal: AbortSignal) => Promise<T>): Promise<T> {
  const stop = new AbortController();
  const interrupt = () => stop.abort(new Error("desktop test build interrupted"));
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  try {
    const result = await build(stop.signal);
    stop.signal.throwIfAborted();
    return result;
  }
  finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}

/** Run a build step with inherited output; with `capture`, its standard output is
 * returned and its progress on standard error stays visible. */
export async function runStep(program: string, args: readonly string[], options: { env: Environment; cwd: NativePath; capture?: boolean; signal?: AbortSignal }): Promise<string> {
  if (!options.signal) {
    const code = await runVisible(program, args, options.env, options.cwd);
    if (code !== 0) throw new Error(`${program} ${args.join(" ")} exited ${code}`);
    return "";
  }
  const built = await fixtureRunner({ program, args: [...args], cwd: options.cwd, env: options.env }, options.signal,
    { stdout: !options.capture, stderr: true });
  if (built.code !== 0) throw new Error(`${program} ${args.join(" ")} exited ${built.code}`);
  return built.stdout;
}

/** Run a child runner with inherited output. An interrupt of this process is reported
 * through `interrupted` and reaches the child as a request to shut down, never as a forced
 * kill: the child's service must stop its app before the caller releases fixtures, and
 * the child's exit is always awaited. On Unix the signal is passed on. On Windows the
 * console already delivers the same Ctrl+C to the child, and Node can only emulate a
 * signal there by terminating the process, so nothing is sent. */
export async function runForwardingInterrupts(
  program: string, args: readonly string[], env: Environment, cwd: NativePath, interrupted: () => void,
  owner?: () => number | null,
): Promise<number> {
  const child = spawn(program, args, { env, cwd, stdio: "inherit" });
  // On Linux the child is a session wrapper (dbus-run-session, xvfb-run) that passes no
  // signal on, so the request goes to the runner it wraps once that runner has said who it is.
  let exited = false;
  child.once("exit", () => { exited = true; });
  /** Signal the wrapped runner; until it has said who it is, keep trying while the wrapper
   * lives, so an interrupt early in its start still reaches it rather than the wrapper. */
  const deliver = (signal: NodeJS.Signals, tries: number) => {
    if (exited) return;
    const runner = owner ? owner() : null;
    if (runner !== null) {
      try { process.kill(runner, signal); return; }
      catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
    } else if (owner && tries > 0) {
      setTimeout(() => deliver(signal, tries - 1), 50);
      return;
    }
    child.kill(signal);
  };
  // Long enough for the wrapped runner to start and claim its candidate.
  const OWNER_TRIES = 1200;
  const forward = (signal: NodeJS.Signals) => () => {
    interrupted();
    if (WINDOWS) return;
    deliver(signal, OWNER_TRIES);
  };
  const interrupt = forward("SIGINT");
  const terminate = forward("SIGTERM");
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", terminate);
  try {
    let failure: Error | undefined;
    child.once("error", (error) => { failure = error; });
    const { code, signal } = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done) => {
      child.once("close", (code, signal) => done({ code, signal }));
    });
    if (failure) throw failure;
    if (signal) throw new Error(`runner terminated by ${signal}`);
    return code ?? 1;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", terminate);
  }
}

/** A person's foreground program: keep its real display/environment and output, forward
 * interrupts, and await its exit before the caller deletes its build inputs. Unix children
 * get an owned group so an interrupted build does not leave its compiler children behind. */
export async function runVisible(program: string, args: readonly string[], env: Environment, cwd: NativePath): Promise<number> {
  const child = spawn(program, args, { env, cwd, stdio: "inherit", detached: !WINDOWS });
  const forward = (signal: NodeJS.Signals) => () => {
    if (child.pid === undefined) return;
    if (WINDOWS) {
      // Node has no Unix signals on Windows; kill requests native termination.
      child.kill(signal);
    } else {
      try { process.kill(-child.pid, signal); }
      catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
    }
  };
  const interrupt = forward("SIGINT");
  const terminate = forward("SIGTERM");
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", terminate);
  try {
    return await new Promise<number>((done, fail) => {
      child.once("error", fail);
      child.once("close", (code, signal) => done(code ?? (signal ? 128 + constants.signals[signal] : 1)));
    });
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", terminate);
  }
}

export function setExitCode(code: number): void {
  process.exitCode = code;
}

/** Wrap the entire owner, not an app/driver or an individual WebDriver command. */
export function runnerCommand(args: string[], inherited: Environment = process.env): {
  program: string; args: string[]; env: Environment;
} {
  const env: Environment = { ...inherited };
  if (!LINUX) return { program: process.execPath, args, env };
  delete env.WAYLAND_DISPLAY;
  return {
    program: "dbus-run-session",
    args: ["--", "xvfb-run", "-a", process.execPath, ...args],
    env: { ...env, GDK_BACKEND: "x11", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" },
  };
}

type TtyOutput = { stdout: boolean; stderr: boolean };

/** Do not remove inputs a child may still be using when a join was not confirmed. */
export class ProcessTreeNotJoined extends Error {}

/** Supervise a build or startup regression fixture, never a service-owned WDIO app.
 * Terminate and join the full process tree, including descendants that closed their pipes. */
export async function fixtureRunner(command: ReturnType<typeof runnerCommand> & { cwd?: NativePath }, signal: AbortSignal,
  visible?: TtyOutput): Promise<{
  code: number | null; stdout: string; stderr: string;
}> {
  signal.throwIfAborted();
  if (WINDOWS) return windowsFixtureRunner(command, signal, visible);
  const child = spawn(command.program, command.args, {
    cwd: command.cwd ?? resolve(import.meta.dirname, ".."), env: command.env,
    detached: true, stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  const failures: unknown[] = [];
  child.stdout.setEncoding("utf8").on("data", (chunk: string) => { if (visible?.stdout) process.stdout.write(chunk); else stdout += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk: string) => { if (visible?.stderr) process.stderr.write(chunk); else stderr += chunk; });
  // Abort and spawn errors can precede close; neither releases fixture ownership.
  child.on("error", (error) => { failures.push(error); });
  const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done) => {
    child.once("close", (code, signal) => done({ code, signal }));
  });
  let cancellation: Promise<void> | undefined;
  const cancel = () => {
    cancellation ??= stopTree(child).catch((error: unknown) => {
      failures.push(new ProcessTreeNotJoined("fixture process group teardown was not confirmed", { cause: error }));
    });
  };
  signal.addEventListener("abort", cancel, { once: true });
  if (signal.aborted) cancel();
  try {
    const outcome = await closed;
    // A closed wrapper can leave descendants that closed their inherited pipes.
    // Always settle its owned tree; an abort remains attached throughout teardown.
    cancel();
    await cancellation;
    if (signal.aborted) failures.unshift(signal.reason);
    else if (outcome.signal) failures.push(new Error(`fixture runner terminated by ${outcome.signal}`));
    if (failures.length === 1) throw failures[0];
    if (failures.some((error) => error instanceof ProcessTreeNotJoined)) {
      throw new ProcessTreeNotJoined("failed while stopping the fixture process group", { cause: new AggregateError(failures) });
    }
    if (failures.length > 1) throw new AggregateError(failures, "fixture runner failed or could not be cancelled");
    return { code: outcome.code, stdout, stderr };
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}

/** Stop and join a Unix process group even if the initial child already exited. */
async function stopTree(child: ChildProcess): Promise<void> {
  if (child.pid === undefined) return;
  const group = -child.pid;
  const exists = () => {
    try { process.kill(group, 0); return true; }
    catch (error) { if ((error as NodeJS.ErrnoException).code === "ESRCH") return false; throw error; }
  };
  try { process.kill(group, "SIGKILL"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
  const deadline = Date.now() + 30000;
  while (exists()) {
    if (Date.now() > deadline) throw new Error(`fixture runner process group ${child.pid} outlived SIGKILL`);
    await new Promise((done) => setTimeout(done, 50));
  }
}

/** The job, not the root PID or its inherited pipes, owns Windows descendants.
 * Gate the starter before it can create any descendants or exit. */
async function windowsFixtureRunner(command: ReturnType<typeof runnerCommand> & { cwd?: NativePath; owner?: NativePath; holdDescendants?: boolean }, signal: AbortSignal,
  visible?: TtyOutput) {
  const root = newDirectory(scratchDirectory(), "fixture-job-");
  let retained = false;
  const script = nativePath(root, "job.ps1");
  const input = nativePath(root, "command.json");
  const output = nativePath(root, "outcome.json");
  const starter = nativePath(root, "starter.mjs");
  try {
    writeJson(input, { program: command.program, args: command.args, holdDescendants: command.holdDescendants ?? false });
    // A Store-packaged pwsh starter can launch children outside its assigned job.
    // The same Node executable that runs the suite makes an ordinary Win32 spawn.
    writeText(starter, String.raw`import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";

process.exitCode = 1;
const gate = createInterface({ input: process.stdin });
for await (const line of gate) {
  if (line !== "start") break;
  const { program, args } = JSON.parse(readFileSync(process.argv[2], "utf8"));
  const child = spawn(program, args, { stdio: ["ignore", "inherit", "inherit"] });
  const [code] = await once(child, "exit");
  process.exitCode = code ?? 1;
  break;
}
`);
    writeText(script, String.raw`param([string]$InputPath, [string]$OutputPath, [string]$NodePath, [string]$StarterPath)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class FixtureJob {
  [StructLayout(LayoutKind.Sequential)] struct Limits {
    public long ProcessTime, JobTime; public uint Flags;
    public UIntPtr MinWs, MaxWs; public uint MaxProcesses; public UIntPtr Affinity;
    public uint Priority, Scheduling;
    public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
    public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
  }
  [StructLayout(LayoutKind.Sequential)] struct Accounting {
    public long User, Kernel, PeriodUser, PeriodKernel;
    public uint PageFaults, Total, Active, Terminated;
  }
  [DllImport("kernel32.dll", SetLastError=true)] static extern SafeFileHandle CreateJobObject(IntPtr attrs, IntPtr name);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(SafeFileHandle job, int kind, ref Limits limits, uint length);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(SafeFileHandle job, int kind, out Accounting result, uint length, IntPtr returned);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(SafeFileHandle job, IntPtr process);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(SafeFileHandle job, uint code);
  static void Check(bool ok) { if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
  public static SafeFileHandle Create() {
    var job = CreateJobObject(IntPtr.Zero, IntPtr.Zero);
    Check(!job.IsInvalid);
    try {
      var limits = new Limits { Flags = 0x2000 }; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
      Check(SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf<Limits>()));
      return job;
    } catch { job.Dispose(); throw; }
  }
  public static void Attach(SafeFileHandle job, IntPtr process) { Check(AssignProcessToJobObject(job, process)); }
  public static uint Active(SafeFileHandle job) {
    Accounting result;
    Check(QueryInformationJobObject(job, 1, out result, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero));
    return result.Active;
  }
  public static void Terminate(SafeFileHandle job) { Check(TerminateJobObject(job, 1)); }
}
'@
$job = [FixtureJob]::Create()
$starter = $null
try {
  $info = [Diagnostics.ProcessStartInfo]::new($NodePath)
  $info.UseShellExecute = $false
  $info.RedirectStandardInput = $true
  $info.ArgumentList.Add($StarterPath)
  $info.ArgumentList.Add($InputPath)
  $starter = [Diagnostics.Process]::Start($info)
  try { [FixtureJob]::Attach($job, $starter.Handle) }
  catch {
    # Outside the job the starter must not start the command: close its gate and join it.
    $starter.StandardInput.Close()
    $starter.WaitForExit()
    throw
  }
  $cancelled = [IO.StreamReader]::new([Console]::OpenStandardInput()).ReadLineAsync()
  $starter.StandardInput.WriteLine('start')
  $starter.StandardInput.Close()
  $exited = $starter.WaitForExitAsync()
  $null = [Threading.Tasks.Task]::WhenAny([Threading.Tasks.Task[]]@($cancelled, $exited)).GetAwaiter().GetResult()
  $code = if ($exited.IsCompletedSuccessfully) { $starter.ExitCode } else { $null }
  # Chromium can hand off to a child in the same profile. Keep that exact job
  # alive until release or natural exit even if its initial client returned.
  $command = Get-Content -Raw -LiteralPath $InputPath | ConvertFrom-Json
  if ($command.holdDescendants -and $code -eq 0) {
    while (!$cancelled.IsCompleted -and [FixtureJob]::Active($job)) { Start-Sleep -Milliseconds 20 }
  }
  if ([FixtureJob]::Active($job)) { [FixtureJob]::Terminate($job) }
  $watchdog = [Diagnostics.Stopwatch]::StartNew()
  while ([FixtureJob]::Active($job)) {
    if ($watchdog.Elapsed.TotalSeconds -gt 30) { throw 'Fixture job is still active after termination.' }
    Start-Sleep -Milliseconds 20
  }
  [IO.File]::WriteAllText($OutputPath, (ConvertTo-Json -Compress @{code = $code}))
} finally {
  if ($starter) { $starter.StandardInput.Dispose(); $starter.Dispose() }
  $job.Dispose()
}
`);
    const child = spawn("pwsh", ["-NoProfile", "-NonInteractive", "-File", script, "-InputPath", input, "-OutputPath", output,
      "-NodePath", nodeProgram, "-StarterPath", starter], {
      cwd: command.cwd ?? resolve(import.meta.dirname, ".."), env: command.env, stdio: ["pipe", "pipe", "pipe"],
      // With piped stdio, windowsHide keeps the build job owner out of the caller's
      // console. With detached, pwsh can exit before running the script. The build
      // owner cancels by closing stdin so the job owner can still join.
      windowsHide: visible !== undefined,
    });
    let stdout = "";
    let stderr = "";
    let failure: unknown;
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => { if (visible?.stdout) process.stdout.write(chunk); else stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => { if (visible?.stderr) process.stderr.write(chunk); else stderr += chunk; });
    child.on("error", (error) => { failure = error; });
    if (command.owner) child.once("spawn", () => {
      try { writeText(command.owner!, String(child.pid)); }
      catch (error) { failure = error; child.stdin.end(); }
    });
    child.stdin.on("error", (error) => { if (!signal.aborted) failure = error; });
    const cancel = () => { child.stdin.end(); };
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
    try {
      const status = await new Promise<number | null>((done) => child.once("close", done));
      if (status !== 0 || !exists(output)) {
        if (child.pid === undefined && failure) throw failure;
        // A job whose end is unconfirmed may still use the starter and command files.
        retained = true;
        throw new ProcessTreeNotJoined(`Windows fixture job teardown was not confirmed (${status}): ${stderr}; retained inputs at ${root}`, { cause: failure });
      }
      if (failure) throw failure;
      signal.throwIfAborted();
      const { code } = readJson(output) as { code: number | null };
      return { code, stdout, stderr };
    } finally { signal.removeEventListener("abort", cancel); }
  } finally { if (!retained) removeTree(root); }
}

/** pnpm scripts inherit their real CLI entry point. Use it rather than Windows'
 * .cmd shim, which Node cannot spawn without a shell. No command string is parsed. */
export function packageCommand(args: string[]): { program: string; args: string[] } {
  const entry = variable("npm_execpath");
  if (entry && isAbsolute(entry)) {
    const name = basename(entry).toLowerCase();
    if (/^pnpm\.(?:c|m)?js$/.test(name)) return { program: process.execPath, args: [entry, ...args] };
    if (name === "pnpm" || name === "pnpm-native" || name.endsWith(".exe")) return { program: entry, args };
  }
  if (WINDOWS) {
    // A Human tool starts with node, not pnpm, so npm_execpath need not exist. Find
    // a native pnpm or the JS entry behind npm/Corepack's shim without invoking cmd.
    for (const folder of (variable("PATH") ?? "").split(delimiter).filter(Boolean)) {
      const executable = join(folder, "pnpm.exe");
      if (existsSync(executable)) return { program: executable, args };
      for (const entry of ["node_modules/pnpm/bin/pnpm.cjs", "node_modules/corepack/dist/pnpm.js"]) {
        const script = resolve(folder, entry);
        if (existsSync(script)) return { program: process.execPath, args: [script, ...args] };
      }
    }
    throw new Error("pnpm is missing: install pnpm, or start through pnpm so its executable entry point is inherited");
  }
  return { program: "pnpm", args };
}

// ---------------------------------------------------------------------------------
// Loopback network

/** Select distinct OS-assigned loopback endpoints for the upstream embedded service.
 * Release the reservation before service startup; the service owns the actual listeners. */
export async function embeddedPorts(count: number): Promise<number[]> {
  if (!Number.isInteger(count) || count < 1) throw new Error("embedded port count must be a positive integer");
  const sockets: ReturnType<typeof createServer>[] = [];
  const ports: number[] = [];
  const failures: unknown[] = [];
  try {
    for (let index = 0; index < count; index++) {
      const socket = createServer();
      sockets.push(socket);
      await new Promise<void>((done, fail) => {
        socket.once("error", fail);
        socket.listen(0, "127.0.0.1", () => { socket.off("error", fail); done(); });
      });
      const address = socket.address();
      if (!address || typeof address === "string") throw new Error("no embedded WebDriver port allocated");
      ports.push(address.port);
    }
  } catch (error) { failures.push(error); }
  const released = await Promise.allSettled(sockets.filter((socket) => socket.listening).map((socket) =>
    new Promise<void>((done, fail) => socket.close((error) => error ? fail(error) : done()))));
  for (const result of released) if (result.status === "rejected") failures.push(result.reason);
  if (failures.length) throw new AggregateError(failures, "embedded endpoint selection or release failed");
  return ports;
}

/** Pair an explicit driver port with a separate claim socket that stays bound
 * throughout service startup. The driver port is outside the local ephemeral range,
 * so fixture servers that bind port 0 cannot take it during the handoff. */
export async function reserveEmbeddedPorts(count: number): Promise<{
  port: number; close: () => Promise<void>; release: () => Promise<void>;
}[]> {
  const ephemeral = LINUX ? readFileSync("/proc/sys/net/ipv4/ip_local_port_range", "utf8").trim().split(/\s+/).map(Number)
    : MACOS ? [Number(runProgramOk("sysctl", ["-n", "net.inet.ip.portrange.first"])), Number(runProgramOk("sysctl", ["-n", "net.inet.ip.portrange.last"]))]
      : (() => {
        const numbers = [...runProgramOk("netsh", ["int", "ipv4", "show", "dynamicport", "tcp"])
          .matchAll(/:[ \t]*(\d+)[ \t\r]*$/gm)].map((match) => Number(match[1]));
        const [start, count] = numbers;
        return [start, start + count - 1];
      })();
  const [first, last] = ephemeral;
  if (!(first > 0 && last >= first)) throw new Error("unable to determine the runner's ephemeral port range");
  const choices = Array.from({ length: 30000 }, (_, index) => 4096 + index * 2)
    .filter((port) => port + 1 < first || port > last);
  const reservations: Awaited<ReturnType<typeof reserveEmbeddedPorts>> = [];
  try {
    while (reservations.length < count && choices.length) {
      const [port] = choices.splice(randomInt(choices.length), 1);
      let claim: LoopbackServer | undefined;
      try {
        claim = await serveLoopback(undefined, port + 1);
        const listener = await serveLoopback(undefined, port);
        const owned = claim;
        reservations.push({ port, close: listener.close, release: async () => {
          const failures = (await Promise.allSettled([listener.close(), owned.close()]))
            .flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
          if (failures.length) throw new AggregateError(failures, `embedded driver port ${port} release failed`);
        } });
      } catch (error) {
        await claim?.close();
        if ((error as NodeJS.ErrnoException).code !== "EADDRINUSE") throw error;
      }
    }
    if (reservations.length !== count) throw new Error("not enough exclusive ports for the embedded WebDriver service");
    return reservations;
  } catch (error) {
    const failures = (await Promise.allSettled(reservations.map((port) => port.release())))
      .flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
    if (failures.length) throw new AggregateError([error, ...failures], "embedded port reservation and cleanup failed");
    throw error;
  }
}

export type LoopbackServer = { port: number; listening: () => boolean; close: () => Promise<void> };

/** Listen on a loopback TCP port (0: one the OS assigns) and own all accepted peers. */
export function serveLoopback(onConnection: (socket: Socket) => void = (socket) => socket.end(), port = 0): Promise<LoopbackServer> {
  const sockets = new Set<Socket>();
  const server = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    onConnection(socket);
  });
  let closing: Promise<void> | undefined;
  const close = () => closing ??= new Promise<void>((done, fail) => {
    server.close((error) => error ? fail(error) : done());
    for (const socket of sockets) socket.destroy();
  });
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      if (!address || typeof address === "string") return fail(new Error("no loopback port was assigned"));
      done({ port: address.port, listening: () => server.listening, close });
    });
  });
}

export type HttpServer = { url: string; close: () => Promise<void> };

/** Serve HTTP on a loopback port the OS assigns, as http://127.0.0.1:<port>. Closing also
 * ends kept-alive connections, so the port is released when `close` resolves. */
export function serveLoopbackHttp(handle: (request: IncomingMessage, response: ServerResponse) => void): Promise<HttpServer> {
  const server = createHttpServer(handle);
  let closing: Promise<void> | undefined;
  const close = () => closing ??= new Promise<void>((done, fail) => {
    server.close((error) => error ? fail(error) : done());
    server.closeAllConnections();
  });
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      if (!address || typeof address === "string") {
        void close().finally(() => fail(new Error("no loopback port was assigned")));
        return;
      }
      done({ url: `http://127.0.0.1:${address.port}`, close });
    });
  });
}

/** Close every server, reporting all failures together. */
export async function closeServers(servers: readonly HttpServer[], what: string): Promise<void> {
  const results = await Promise.allSettled(servers.map((server) => server.close()));
  const failures = results.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
  if (failures.length) throw new AggregateError(failures, `${what} did not close`);
}

/** Read liveness of a configured embedded WebDriver endpoint without attaching. */
export async function embeddedReady(port: number): Promise<boolean> {
  try {
    const reply = await fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(500) });
    const contents = await reply.json() as { value?: { ready?: boolean } };
    return reply.ok && contents.value?.ready === true;
  } catch { return false; }
}

/** GET `url` as the system browser would, resolving its host name, and return the status. */
export async function httpGet(url: string): Promise<number> {
  const reply = await fetch(url);
  await reply.body?.cancel();
  return reply.status;
}

/** This process's id, as other processes observe it. */
export function processId(): number {
  return process.pid;
}

/** Whether the process `pid` still runs. Only reads: signal 0 checks without signalling.
 * On Unix an exited process the OS has not reaped yet (a zombie) holds nothing and counts
 * as exited; Windows has no such state. */
export function processRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
    throw error;
  }
  if (LINUX) {
    const stat = readTextIfExists(`/proc/${pid}/stat` as NativePath);
    // The state follows the parenthesized command name, which may itself contain ")".
    return stat !== null && stat[stat.lastIndexOf(")") + 2] !== "Z";
  }
  if (MACOS) {
    const state = spawnSync("ps", ["-o", "stat=", "-p", String(pid)], { encoding: "utf8" });
    if (state.error) throw state.error;
    if (state.status === 0 && state.stdout.trim()) return !state.stdout.trim().startsWith("Z");
    // ps exits 1 when the PID disappeared during observation. Recheck the PID rather
    // than treating an arbitrary ps failure as confirmation of exit.
    try { process.kill(pid, 0); }
    catch (error) { if ((error as NodeJS.ErrnoException).code === "ESRCH") return false; throw error; }
    throw new Error(`cannot observe live fixture PID ${pid}: ps exited ${state.status}: ${state.stderr}`);
  }
  return true;
}

export type Database = DatabaseSync;

/** Change a file-backed SQLite database whose handle never outlives the call. */
export function updateDatabase(path: NativePath, change: (database: Database) => void): void {
  const database = new DatabaseSync(path);
  try {
    database.exec("PRAGMA busy_timeout=5000");
    change(database);
  } finally { database.close(); }
}

/** Fail unless the product's fixed loopback `port` can be bound right now. */
export async function requireFreeLoopbackPort(port: number, why: string): Promise<void> {
  try { await (await serveLoopback(undefined, port)).close(); }
  catch (cause) { throw new Error(why, { cause }); }
}

/** Connect to a loopback port and read until the peer ends the connection. A refused or
 * reset connection is `null`: nothing serves the port. `timeout` bounds a stalled peer. */
export function readLoopback(port: number, timeout: number): Promise<string | null> {
  return new Promise((done, fail) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    let output = "";
    socket.setEncoding("utf8");
    socket.setTimeout(timeout, () => socket.destroy(new Error("loopback read timed out")));
    socket.on("data", (chunk: string) => { output += chunk; });
    socket.once("end", () => done(output));
    socket.once("error", (error: NodeJS.ErrnoException) => {
      if (["ECONNREFUSED", "ECONNRESET"].includes(error.code ?? "")) done(null);
      else fail(error);
    });
    socket.once("close", () => socket.destroy());
  });
}

/** Own a child whose parent must exit while the child remains live: a runner
 * regression seam, not an app/driver launcher. */
export function startOrphanFixture(program: string, args: string[]): Promise<void> {
  const child = spawn(program, args, { stdio: "ignore" });
  return new Promise((done, fail) => {
    child.once("error", fail);
    child.once("spawn", () => { child.unref(); done(); });
  });
}

/** Connect to a loopback port, send `text` and end the connection. */
export function sendLoopback(port: number, text: string): Promise<void> {
  return new Promise((done, fail) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    socket.once("error", fail);
    socket.on("connect", () => socket.end(text, () => done()));
  });
}

// ---------------------------------------------------------------------------------
// Runner machine and native session

/** Native display/session inputs survive fixture auth/profile sanitization. */
export function harnessEnvironment(): Record<string, string> {
  if (!LINUX) return {};
  const env: Record<string, string> = {
    GDK_BACKEND: "x11", GTK_IM_MODULE: "gtk-im-context-simple", XMODIFIERS: "",
    GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1",
  };
  for (const name of ["DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS"]) {
    const value = process.env[name];
    if (value !== undefined) env[name] = value;
  }
  return env;
}

/** The general pasteboard on macOS cannot be redirected into a fixture's profile. */
export function assertClipboardDesktop(): void {
  if (MACOS && !(variable("GITHUB_ACTIONS") === "true" && variable("RUNNER_ENVIRONMENT") === "github-hosted" && variable("RUNNER_OS") === "macOS")) {
    throw new Error("Native clipboard E2E requires a disposable GitHub-hosted macOS runner; the user's general pasteboard must stay untouched");
  }
}

export function assertRunnerMachine(): void {
  if (WINDOWS && !(variable("GITHUB_ACTIONS") === "true"
    && variable("RUNNER_ENVIRONMENT") === "github-hosted" && variable("RUNNER_OS") === "Windows")) {
    throw new Error("Windows desktop E2E requires a disposable GitHub-hosted Windows runner; its default URL handler cannot be isolated per app instance");
  }
}

/** Test the address actually belongs to the daemon under our wrapper. Merely
 * seeing a daemon alongside a user-supplied bus address is insufficient. */
function privateLinuxBus(pid: number, address: string): boolean {
  const [transport] = address.split(",");
  const actual = /^unix:(path|abstract)=(.+)$/.exec(transport);
  if (!actual) return false;
  const target = `${actual[1] === "abstract" ? "@" : ""}${decodeURIComponent(actual[2])}`;
  const inodes = new Set<string>();
  for (const line of readFileSync("/proc/net/unix", "utf8").split("\n")) {
    const match = /^(?:\S+\s+){6}(\d+)\s+(.+)$/.exec(line);
    if (match?.[2] === target) inodes.add(`socket:[${match[1]}]`);
  }
  try {
    const directory = `/proc/${pid}/fd`;
    return readdirSync(directory).some((fd) => {
      try { return inodes.has(readlinkSync(join(directory, fd))); }
      catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return false; throw error; }
    });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}

/** Verify the app launcher belongs to the private sessions, not merely that
 * its environment looks like an X11 login. Workers inherit the same ancestry. */
export function assertHarnessPlatform(): void {
  assertRunnerMachine();
  if (!LINUX) return;
  const read = (pid: number, file: string) => readTextIfExists(nativePath(`/proc/${pid}`, file)) ?? "";
  const command = (pid: number) => read(pid, "cmdline").split("\0");
  const parent = (pid: number) => Number(read(pid, "status").match(/^PPid:\s+(\d+)/m)?.[1] ?? 0);
  const children = (pid: number) => read(pid, `task/${pid}/children`).trim().split(/\s+/).filter(Boolean).map(Number);
  const { DISPLAY: display, XAUTHORITY: auth, DBUS_SESSION_BUS_ADDRESS: bus, GDK_BACKEND: backend, WAYLAND_DISPLAY: wayland } = process.env;
  if (display && auth && bus && backend === "x11" && !wayland) {
    for (let pid = parent(process.pid); pid > 1; pid = parent(pid)) {
      const args = command(pid);
      if (basename(args[1] ?? "") !== "xvfb-run" || !args.includes("-a")) continue;
      const session = parent(pid);
      if (basename(command(session)[0]) !== "dbus-run-session") continue;
      const xvfb = children(pid).some((child) => {
        const [program, screen, ...flags] = command(child);
        return basename(program) === "Xvfb" && screen === display && flags.includes("-auth") && flags[flags.indexOf("-auth") + 1] === auth;
      });
      const daemon = children(session).some((child) => basename(command(child)[0]) === "dbus-daemon" && privateLinuxBus(child, bus));
      if (xvfb && daemon) return;
    }
  }
  throw new Error("Linux desktop E2E requires the whole runner inside its own dbus-run-session -- xvfb-run -a, GDK_BACKEND=x11 and no WAYLAND_DISPLAY");
}

/** Some origins are known at setup, others are announced by the owned launcher. */
export type BrowserEntry = string | { dynamic: true } | { file: NativePath };

/** A restarted cleanup may delete browser inputs only after every job owner has exited. */
function browserJobsAlive(root: NativePath): boolean {
  const owners = nativePath(root, "browser-jobs");
  return exists(owners) && listFiles(owners).filter((file) => file.endsWith(".pid")).some((file) => {
    const pid = Number(readText(file));
    if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error(`invalid browser job record ${file}`);
    return processRunning(pid);
  });
}

export async function releaseRecordedBrowser(root: NativePath): Promise<void> {
  if (browserJobsAlive(root)) throw new Error(`a native browser job still owns ${root}`);
}

function programPath(env: Environment): string[] {
  const key = Object.keys(env).find((name) => sameVariable(name, "PATH"));
  return (key === undefined ? "" : env[key] ?? "").split(delimiter).filter(Boolean);
}

/** Add a declared, native test command directory to an app's private PATH. */
export function commandDirectory(env: Environment, bin: NativePath): Environment {
  return withVariable({ ...env }, "PATH", [bin, ...programPath(env)].join(delimiter));
}

/** How many test apps run at once. A Windows desktop has one foreground window, and WebView2
 * gives no focus events to a page whose window is not it (focusing the WebView alone does not
 * do it): an app beside another one missed its own focus changes. The foreground window is the
 * one per-desktop resource every instance needs (E2E-ISOLATED), so Windows runs one app at a time. */
export const appsAtOnce = WINDOWS ? 1 : 2;

/** Give the scenario's app window the system focus before it starts, on Windows (see
 * appsAtOnce); other systems deliver focus events to a page in any window. */
export async function takeSystemFocus(browser: WebdriverIO.Browser): Promise<void> {
  if (!WINDOWS) return;
  await browser.switchToWindow(await browser.getWindowHandle());
  await browser.waitUntil(() => browser.execute(() => document.hasFocus()),
    { timeout: 15000, timeoutMsg: "setup failed: the app window did not take the system focus" });
}

/** Ask the public Tauri window API to perform a genuine close request. The
 * pristine provider's DELETE /window force-destroys and bypasses this app event. */
export async function requestWindowClose(browser: WebdriverIO.Browser): Promise<void> {
  const error = await browser.executeAsync((done) => {
    const app = window as unknown as { __TAURI__: { window: { getCurrentWindow: () => { close: () => Promise<void> } } } };
    app.__TAURI__.window.getCurrentWindow().close().then(
      () => done(null),
      (failure: unknown) => done(String(failure)),
    );
  });
  if (error !== null) throw new Error(`native window close failed: ${String(error)}`);
}

const browserHandoffLog = (root: NativePath) => nativePath(root, "browser-handoffs.jsonl");

/** URLs received by this fixture's desktop opener, independently of visits to the site. */
export function browserHandoffs(root: NativePath): string[] {
  return (readTextIfExists(browserHandoffLog(root)) ?? "").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);
}

/** The release fixture records a page after load, or owns the browser's completed download.
 * The Unix browser stand-in instead records each handoff only after consuming its HTTP body. */
export function browserTransferComplete(root: NativePath, url: string, after: number, transfer:
  { pageLoaded: NativePath } | { download: string; body: string }): boolean {
  if (!browserHandoffs(root).slice(after).includes(url)) return false;
  if (!WINDOWS) return true;
  if ("pageLoaded" in transfer) return exists(transfer.pageLoaded);
  return readTextIfExists(nativePath(root, "browser-downloads", transfer.download)) === transfer.body;
}

/** The system browser of a case that opens `entry`, an address on its private loopback server,
 * or one below it. Unix desktop openers are declared fixture executables performing the real
 * HTTP GET. */
export async function browserEnvironment(root: NativePath, env: Environment, entry: BrowserEntry): Promise<{
  env: Environment; release?: () => Promise<void>;
}> {
  const file = typeof entry !== "string" && "file" in entry ? entry.file : undefined;
  if (file && !isInside(root, file)) throw new Error("desktop handoff file must be owned by the fixture");
  if (WINDOWS) return ownedWindowsBrowser(root, env, entry, file);
  const bin = makeDirectory(nativePath(root, "browser-bin"));
  const program = nativePath(bin, "open-url.mjs");
  const entries = nativePath(root, "browser-entries.jsonl");
  const stop = new AbortController();
  const owners = makeDirectory(nativePath(root, "browser-jobs"), { private: true });
  const jobs: Promise<void>[] = [];
  const server = await serveLoopbackHttp((request, response) => {
    const owner = Number(request.headers["x-opener-pid"]);
    if (request.method !== "POST" || request.url !== "/open" || stop.signal.aborted || !Number.isSafeInteger(owner) || owner <= 0) {
      response.writeHead(404).end();
      return;
    }
    const job = (async () => {
      replaceText(nativePath(owners, `${owner}.pid`), String(owner));
      let target = "";
      for await (const chunk of request) {
        target += String(chunk);
        if (target.length > 16384) throw new Error("browser request is too long");
      }
      let url: URL;
      if (file) {
        if (canonicalPath(asNativePath(target)) !== canonicalPath(file)) throw new Error("unexpected desktop handoff file");
        const redirect = readText(file).match(/content="0;url=(http:\/\/127\.0\.0\.1:[0-9]+\/file-opened)"/i)?.[1];
        if (!redirect) throw new Error("fixture handoff page has no owned redirect");
        url = new URL(redirect);
      } else {
        url = new URL(target);
        const agreed = typeof entry === "string" ? new URL(entry).origin === url.origin
          : (readTextIfExists(entries) ?? "").split("\n").includes(url.href);
        if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || !agreed) {
          throw new Error("the handoff did not name the owned browser origin");
        }
      }
      const reply = await fetch(url, { signal: AbortSignal.any([stop.signal, AbortSignal.timeout(30000)]) });
      await reply.arrayBuffer();
      if (!reply.ok) throw new Error(`browser HTTP ${reply.status}`);
      if (!file) appendText(browserHandoffLog(root), `${JSON.stringify(target)}\n`);
      response.writeHead(204).end();
    })().catch((error: unknown) => {
      if (!response.headersSent && !response.destroyed) response.writeHead(500).end(String(error));
      throw error;
    });
    jobs.push(job);
    void job.catch(() => {}); // joined by the owning fixture even if the app ignores opener failure
  });
  try {
    writeText(program, `const reply = await fetch(${JSON.stringify(`${server.url}/open`)}, {
  method: "POST", headers: { "x-opener-pid": String(process.pid) }, body: process.argv[2],
  signal: AbortSignal.timeout(30000),
});
if (!reply.ok) throw new Error(await reply.text());
`);
    const opener = MACOS ? "open" : "xdg-open";
    writeFileSync(join(bin, opener), '#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$DESKTOP_TEST_BROWSER_PROGRAM" "$@"\n', { mode: 0o755 });
  } catch (error) {
    await server.close();
    throw error;
  }
  return {
    env: { ...commandDirectory(env, bin), DESKTOP_TEST_NODE: nodeProgram, DESKTOP_TEST_BROWSER_PROGRAM: program,
      DESKTOP_TEST_BROWSER_ENTRIES: entries },
    release: async () => {
      stop.abort();
      const outcomes = await Promise.allSettled([server.close(), ...jobs]);
      const failures = outcomes.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
      const deadline = Date.now() + 30000;
      while (browserJobsAlive(root)) {
        if (Date.now() >= deadline) {
          failures.push(new Error(`fixture browser opener still owns ${root}`));
          break;
        }
        await new Promise((done) => setTimeout(done, 25));
      }
      if (failures.length) throw new AggregateError(failures, "owned browser handoffs failed");
    },
  };
}

/** Windows takes the declared external browser program through the test-only
 * opener seam. Supply a real Edge profile and native job; do not use an OS-wide
 * default association or find another user's browser by process-name scans. */
async function ownedWindowsBrowser(root: NativePath, env: Environment, entry: BrowserEntry, file: NativePath | undefined) {
  assertRunnerMachine();
  const edge = [variable("ProgramFiles(x86)"), variable("ProgramFiles")].filter((value): value is string => !!value)
    .map((directory) => nativePath(directory, "Microsoft", "Edge", "Application", "msedge.exe")).find(exists);
  if (!edge) throw new Error("the disposable Windows runner has no declared Edge browser");
  const profile = makeDirectory(nativePath(root, "edge-profile"), { private: true });
  const downloads = makeDirectory(nativePath(root, "browser-downloads"), { private: true });
  const defaults = makeDirectory(nativePath(profile, "Default"));
  writeJson(nativePath(defaults, "Preferences"), { download: { default_directory: downloads, prompt_for_download: false } });
  const owners = makeDirectory(nativePath(root, "browser-jobs"), { private: true });
  const program = nativePath(root, "owned-browser.mjs");
  const entries = nativePath(root, "browser-entries.jsonl");
  const stop = new AbortController();
  const released = new Error("fixture browser released");
  const jobs: Promise<void>[] = [];
  const requests: Promise<void>[] = [];
  const server = await serveLoopbackHttp((request, response) => {
    if (request.method !== "POST" || request.url !== "/open" || stop.signal.aborted) {
      response.writeHead(404).end(); return;
    }
    const admitted = (async () => {
      let target = "";
      for await (const chunk of request) {
        target += String(chunk);
        if (target.length > 16384) throw new Error("browser request is too long");
      }
      // Closing the listener interrupts bodies in flight. No request still
      // reading its body at the teardown boundary can acquire an Edge job.
      stop.signal.throwIfAborted();
      const launch = file ? canonicalPath(asNativePath(target)) : target;
      if (file) {
        if (launch !== canonicalPath(file)) throw new Error("the handoff did not name the owned file");
      } else {
        const address = new URL(target);
        const agreed = typeof entry === "string" ? new URL(entry).origin === address.origin :
          (readTextIfExists(entries) ?? "").split("\n").includes(address.href);
        if (address.protocol !== "http:" || address.hostname !== "127.0.0.1" || !agreed) throw new Error("the handoff did not name the owned browser origin");
        appendText(browserHandoffLog(root), `${JSON.stringify(target)}\n`);
      }
      const args = [`--user-data-dir=${portablePath(profile)}`, "--no-first-run", "--no-default-browser-check", "--disable-background-mode", launch];
      const owner = nativePath(owners, `${jobs.length}.pid`);
      const job = windowsFixtureRunner({ program: edge, args, env, owner, holdDescendants: true }, stop.signal).then((done) => {
        if (done.code !== 0) throw new Error(`fixture Edge exited ${done.code}: ${done.stderr}`);
      }).catch((error: unknown) => { if (error !== released) throw error; });
      jobs.push(job);
      void job.catch(() => {}); // handled collectively by release
      response.writeHead(204).end();
    })().catch((error: unknown) => {
      if (error === released || (stop.signal.aborted && !request.complete)) return;
      if (!response.headersSent && !response.destroyed) response.writeHead(500).end(String(error));
      throw error;
    });
    requests.push(admitted);
    void admitted.catch(() => {}); // joined by the owning fixture
  });
  try {
    writeText(program, `const reply = await fetch(${JSON.stringify(`${server.url}/open`)}, { method: "POST", body: process.argv[2] });
if (!reply.ok) throw new Error(await reply.text());
`);
  } catch (error) {
    await server.close();
    throw error;
  }
  return {
    env: { ...env, ARTIFACTIZE_E2E_BROWSER_NODE: nodeProgram, ARTIFACTIZE_E2E_BROWSER_PROGRAM: program,
      // The external auth command receives the author's environment, without ARTIFACTIZE_*.
      DESKTOP_TEST_BROWSER_NODE: nodeProgram, DESKTOP_TEST_BROWSER_CLIENT: program,
      DESKTOP_TEST_BROWSER_ENTRIES: entries },
    release: async () => {
      stop.abort(released);
      const outcomes = await Promise.allSettled([server.close(), ...requests, ...jobs]);
      const failures = outcomes.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
      try { await releaseRecordedBrowser(root); } catch (error) { failures.push(error); }
      if (failures.length) throw new AggregateError(failures, "owned Edge browser jobs failed");
    },
  };
}

/** A scripted Claude Code (`claude auth status|login|logout`) first on the app's PATH, keeping
 * its sign-in as the file `signed-in` in `root`. `login` serves a loopback origin, prints its
 * sign-in URL there and signs in when the browser returns to its `/callback` or the code
 * `good-code` is pasted; any other path of it answers while it runs, and its input closing
 * stops it. Claude Code's own configuration points into `root`, never the user's. */
export function claudeCodeEnvironment(root: NativePath, env: Environment, watchdogMs = 120000): Environment {
  const bin = makeDirectory(nativePath(root, "claude-bin"));
  const program = nativePath(bin, "claude.mjs");
  const logins = makeDirectory(nativePath(root, "claude-logins"), { private: true });
  writeText(program, `import { appendFileSync, existsSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { join } from "node:path";
const [group, command] = process.argv.slice(2);
const signedIn = process.env.DESKTOP_TEST_CLAUDE_SIGNED_IN;
const vault = process.env.DESKTOP_TEST_CLAUDE_VAULT;
const account = (path) => fetch(vault + path);
const say = (text) => process.stdout.write(text);
if (group !== "auth") process.exit(2);
// An older Claude Code, which has no auth command: it reports nothing and says why.
if (existsSync(process.env.DESKTOP_TEST_CLAUDE_BROKEN)) {
  process.stderr.write("error: unknown command 'auth'\\n");
  process.exit(1);
}
if (command === "status") {
  const signed = vault ? !!(await (await account("/credential")).text()) : existsSync(signedIn);
  say(JSON.stringify(signed
    ? { loggedIn: true, authMethod: "claude.ai", subscriptionType: "max", email: "person@example.com" }
    : { loggedIn: false, authMethod: "none" }) + "\\n");
  process.exit(signed ? 0 : 1);
}
if (command === "logout") {
  if (vault) {
    if (!(await account("/logout")).ok) process.exit(1);
  } else rmSync(signedIn, { force: true });
  say("Successfully logged out.\\n");
  process.exit(0);
}
if (command !== "login") process.exit(2);
// Admit the actual login process before allocating any resources. Teardown can
// atomically close admission; late starts then cannot serve or open anything.
const logins = process.env.DESKTOP_TEST_CLAUDE_LOGINS;
writeFileSync(join(logins, process.pid + "-" + randomUUID() + ".pid"), "", { flag: "wx" });
setInterval(() => { if (!existsSync(logins)) process.exit(1); }, 100).unref();
setTimeout(() => process.exit(1), ${watchdogMs}).unref();
let finishing = false;
const finish = async () => {
  if (finishing) return;
  finishing = true;
  if (vault) {
    if (!(await account("/login")).ok) process.exit(1);
  } else writeFileSync(signedIn, "claude.ai");
  say("Login successful.\\n");
  process.exit(0);
};
const server = createServer((request, response) => {
  // What the browser visits is recorded, for the case to see that it opened.
  if (request.url.startsWith("/oauth/authorize")) appendFileSync(process.env.DESKTOP_TEST_CLAUDE_VISITS, request.url + "\\n");
  response.end("signed in");
  if (request.url.startsWith("/callback")) finish();
});
server.listen(0, "127.0.0.1", () => {
  const address = \`http://127.0.0.1:\${server.address().port}\`;
  appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OWNERS, JSON.stringify({ address, pid: process.pid }) + "\\n");
  const url = \`\${address}/oauth/authorize?code=true\`;
  say(\`Opening browser to sign in\\nIf the browser didn't open, visit: \${url}\\nPaste code here if prompted > \`);
  // As Claude Code does, it opens the system browser itself: only the case's fixture browser,
  // never the person's own.
  if (process.env.DESKTOP_TEST_BROWSER_ENTRIES) {
    appendFileSync(process.env.DESKTOP_TEST_BROWSER_ENTRIES, url + "\\n");
  }
  const failed = (reason) => {
    const text = "Claude fixture browser opener failed: " + String(reason);
    appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OPEN_ERRORS, text + "\\n");
    process.stderr.write(text + "\\n");
    process.exit(1);
  };
  const opener = process.env.DESKTOP_TEST_BROWSER_CLIENT
    ? spawn(process.env.DESKTOP_TEST_BROWSER_NODE, [process.env.DESKTOP_TEST_BROWSER_CLIENT, url], { stdio: "inherit" })
    : process.env.DESKTOP_TEST_BROWSER_PROGRAM
      ? spawn(${JSON.stringify(MACOS ? "open" : "xdg-open")}, [url], { stdio: "inherit" })
      : null;
  if (!opener) return failed("no owned browser client was configured");
  opener.on("error", failed);
  // Admitted from its spawn, like the login itself: teardown joins it even if it never
  // reaches the fixture's server.
  if (opener.pid !== undefined) {
    try { writeFileSync(join(logins, opener.pid + "-" + randomUUID() + ".pid"), "", { flag: "wx" }); }
    catch (error) { opener.kill(); failed(error); }
  }
  opener.on("close", (code, signal) => {
    if (code !== 0) return failed(signal ?? "exit " + code);
    appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OPENED, url + "\\n");
  });
});
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (text) => {
  input += text;
  if (!input.includes("\\n")) return;
  if (input.split("\\n")[0].trim() === "good-code") { void finish(); return; }
  process.stderr.write("Login failed: That code was not accepted.\\n");
  process.exit(1);
});
process.stdin.on("end", () => { if (!finishing) process.exit(1); });
`);
  // Paths are arguments, never interpreted as shell source.
  if (WINDOWS) {
    writeText(nativePath(bin, "claude.cmd"), '@"%DESKTOP_TEST_NODE%" "%DESKTOP_TEST_CLAUDE_PROGRAM%" %*\r\n');
  } else {
    writeFileSync(join(bin, "claude"), '#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$DESKTOP_TEST_CLAUDE_PROGRAM" "$@"\n', { mode: 0o755 });
  }
  // The fixture's Claude Code is the only one the app can find: a folder of PATH holding
  // another one, such as the developer's own, is left out, so hiding the fixture's leaves none.
  const others = programPath(env).filter((folder) => !holdsClaudeCode(folder));
  return {
    ...withVariable({ ...env }, "PATH", [bin, ...others].join(delimiter)),
    DESKTOP_TEST_NODE: process.execPath,
    DESKTOP_TEST_CLAUDE_PROGRAM: program,
    DESKTOP_TEST_CLAUDE_SIGNED_IN: nativePath(root, "signed-in"),
    DESKTOP_TEST_CLAUDE_BROKEN: nativePath(root, "claude-broken"),
    DESKTOP_TEST_CLAUDE_VISITS: nativePath(root, "claude-visits"),
    DESKTOP_TEST_CLAUDE_OWNERS: nativePath(root, "claude-owners.jsonl"),
    DESKTOP_TEST_CLAUDE_LOGINS: logins,
    DESKTOP_TEST_CLAUDE_OPEN_ERRORS: nativePath(root, "claude-browser-errors"),
    DESKTOP_TEST_CLAUDE_OPENED: nativePath(root, "claude-browser-opened"),
    CLAUDE_CONFIG_DIR: makeDirectory(nativePath(root, "claude-config"), { private: true }),
  };
}

/** A result recorded by the actual login command, not just its printed sign-in URL. */
export function claudeBrowserOpened(root: NativePath): string[] {
  const failures = readTextIfExists(nativePath(root, "claude-browser-errors"));
  if (failures) throw new Error(failures.trim());
  return (readTextIfExists(nativePath(root, "claude-browser-opened")) ?? "").split("\n").filter(Boolean);
}

/** Stop and join admitted Claude logins after the app stops, before closing their browser. */
export async function releaseClaudeLogins(root: NativePath): Promise<void> {
  const stopped = nativePath(root, "closed-claude-logins");
  closeDirectory(nativePath(root, "claude-logins"), stopped);
  if (!exists(stopped)) return;
  const pids = listFiles(stopped).map((file) => {
    const pid = Number(/^(\d+)-[a-f0-9-]+\.pid$/.exec(nameOf(file))?.[1]);
    if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error(`invalid Claude login owner: ${file}`);
    return pid;
  });
  const deadline = Date.now() + 15000;
  while (pids.some(processRunning)) {
    if (Date.now() >= deadline) throw new Error(`Claude login still owns fixture ${root}`);
    await new Promise((done) => setTimeout(done, 25));
  }
  removeTree(stopped);
}

/** Read the actual login process that announced this origin while it was live. */
export function claudeLoginOwner(root: NativePath, address: string): number {
  const records = readText(nativePath(root, "claude-owners.jsonl")).split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as { address: string; pid: number });
  const pid = records.findLast((entry) => entry.address === address)?.pid;
  if (pid === undefined || !Number.isSafeInteger(pid) || pid <= 0) throw new Error(`Claude fixture did not record the owner of ${address}`);
  return pid;
}

/** The file name the fixture's Claude Code is found by on PATH. */
const CLAUDE_CODE = WINDOWS ? "claude.cmd" : "claude";

/** Whether `folder` holds a program a shell would run as `claude`. A folder that cannot be
 * read holds none the app could run either. */
function holdsClaudeCode(folder: string): boolean {
  try {
    return readdirSync(folder).some((name) => name.toLowerCase().replace(/\.(cmd|exe|bat|ps1)$/, "") === "claude");
  } catch {
    return false;
  }
}

/** Take the fixture's Claude Code off PATH, or put it back: `claudeCodeEnvironment(root, …)`
 * left no other one there, so the app then finds none. */
export function claudeCodeInstalled(root: NativePath, installed: boolean): void {
  const bin = nativePath(root, "claude-bin");
  const [from, to] = installed ? [`${CLAUDE_CODE}.off`, CLAUDE_CODE] : [CLAUDE_CODE, `${CLAUDE_CODE}.off`];
  renameSync(join(bin, from), join(bin, to));
}

/** The evaluated test-build device reaches only the engine's text-input API. This
 * test-only IPC command is not an application action and is absent from production. */
export async function nativeInput(browser: WebdriverIO.Browser, input:
  | { type: "set" | "commit"; text: string }
  | { type: "cancel" | "copy" | "paste" }): Promise<void> {
  if (input.type === "copy" || input.type === "paste") assertClipboardDesktop();
  // Through the Tauri service's public API, which reaches the command as the page's own
  // `invoke` would.
  try {
    await browser.tauri.execute((tauri, operation: typeof input) => tauri.core.invoke("e2e_input", { input: operation }), input);
  } catch (failure) {
    throw new Error(`native engine input failed: ${String(failure)}`, { cause: failure });
  }
}

/** Native crash reports for this run's exact candidate binary, never other apps or runs.
 * A crashed embedded driver cannot answer WebDriver diagnostics; the OS report survives it. */
export function nativeCrashReports(app: NativePath): string[] {
  if (!MACOS) return [];
  const directory = nativePath(homedir(), "Library", "Logs", "DiagnosticReports");
  if (!exists(directory)) return [];
  const reports: string[] = [];
  for (const name of readdirSync(directory).filter((entry) => entry.startsWith(`${nameOf(app)}-`) && entry.endsWith(".ips"))) {
    const text = readFileSync(nativePath(directory, name), "utf8");
    // Incident reports start with one metadata JSON line, then the crash's JSON object.
    const report = JSON.parse(text.slice(text.indexOf("\n") + 1)) as {
      procPath?: string; pid?: number; exception?: unknown; termination?: unknown;
      faultingThread?: number; threads?: { triggered?: boolean; frames?: unknown[] }[];
      lastExceptionBacktrace?: unknown[];
    };
    if (report.procPath !== portablePath(app)) continue;
    reports.push(JSON.stringify({
      nativeCrash: name, pid: report.pid, exception: report.exception,
      termination: report.termination, lastExceptionBacktrace: report.lastExceptionBacktrace,
      thread: report.threads?.find((thread) => thread.triggered) ?? report.threads?.[report.faultingThread ?? 0],
    }));
  }
  return reports;
}

/** Every non-directory entry of a source tree, including links and special files. */
export function sourceTree(root: NativePath): { path: NativePath; name: string; kind: "file" | "other" }[] {
  const visit = (directory: NativePath, prefix: string): { path: NativePath; name: string; kind: "file" | "other" }[] =>
    readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
      const path = nativePath(directory, entry.name);
      const name = prefix ? textPath(prefix, entry.name) : entry.name;
      return entry.isDirectory() ? visit(path, name) : [{ path, name, kind: entry.isFile() ? "file" : "other" }];
    });
  return visit(root, "").sort((left, right) => left.name.localeCompare(right.name));
}

/** Original bytes, for checksum-based upstream provenance rather than text normalization. */
export function readBytes(path: NativePath): Uint8Array { return readFileSync(path); }

/** Failure-only native/backend diagnostics from this batch's service-owned log files. */
export function serviceFailureLogs(directory: NativePath): string[] {
  // Keep startup and crash causes visible in CI without replaying entire backend logs.
  const tailCharacters = 16 * 1024;
  return sourceTree(directory).filter((item) => item.kind === "file" && item.name.endsWith(".log"))
    .map((item) => `--- service log ${item.name} ---\n${readText(item.path).slice(-tailCharacters)}`);
}
