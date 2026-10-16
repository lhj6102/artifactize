// The desktop E2E platform layer: the only module that names an operating system,
// reads or changes the process environment, builds native paths, touches the file
// system, starts or stops processes, or opens sockets. Every other E2E module calls
// these portable functions, so an OS difference in CI is investigated here first.
// Runner configuration is the only other place where platforms differ; specs contain
// no OS branches. The app/driver lifetime still belongs to WDIO's Tauri service.
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import {
  appendFileSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync,
  realpathSync, renameSync, rmSync, writeFileSync,
} from "node:fs";
import { createServer as createHttpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createConnection, createServer, type Socket } from "node:net";
import { tmpdir } from "node:os";
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

export function copyFile(from: NativePath, to: NativePath): void {
  copyFileSync(from, to);
}

/** Remove a directory tree. Windows briefly keeps files of exited processes open, so
 * a busy file is retried a bounded number of times before the failure is reported. */
export function removeTree(path: NativePath): void {
  // A Windows app's WebView2 helper processes exit after the app and briefly keep the data
  // folder that lives in its fixture; deletion retries with linearly growing pauses (up to a
  // minute in all) while they finish. Elsewhere nothing outlives the joined process.
  rmSync(path, { recursive: true, force: true, maxRetries: WINDOWS ? 30 : 0, retryDelay: 125 });
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
export function runProgramAsync(program: string, args: readonly string[], options: RunOptions = {}): Promise<Completed> {
  return new Promise((done, fail) => {
    const child = spawn(program, args, { env: options.env, cwd: options.cwd, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => { stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => { stderr += chunk; });
    child.once("error", fail);
    child.once("close", (code) => done({ status: code ?? -1, stdout, stderr }));
  });
}

/** Run a build step with inherited output; with `capture`, its standard output is
 * returned and its progress on standard error stays visible. */
export async function runStep(program: string, args: readonly string[], options: { env: Environment; cwd: NativePath; capture?: boolean }): Promise<string> {
  const child = spawn(program, args, { cwd: options.cwd, env: options.env, stdio: ["ignore", options.capture ? "pipe" : 2, "inherit"] });
  let output = "";
  child.stdout?.on("data", (chunk: Buffer) => { output += chunk; });
  const code = await new Promise<number | null>((done, fail) => {
    child.once("error", fail);
    child.once("close", done);
  });
  if (code !== 0) throw new Error(`${program} ${args.join(" ")} exited ${code}`);
  return output;
}

/** Run a child runner with inherited output. An interrupt of this process is reported
 * through `interrupted` and reaches the child as a request to shut down, never as a forced
 * kill: the child's service must stop its app before the caller releases fixtures, and
 * the child's exit is always awaited. On Unix the signal is passed on. On Windows the
 * console already delivers the same Ctrl+C to the child, and Node can only emulate a
 * signal there by terminating the process, so nothing is sent. */
export async function runForwardingInterrupts(
  program: string, args: readonly string[], env: Environment, cwd: NativePath, interrupted: () => void,
): Promise<number> {
  const child = spawn(program, args, { env, cwd, stdio: "inherit" });
  const forward = (signal: NodeJS.Signals) => () => {
    interrupted();
    if (!WINDOWS) child.kill(signal);
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

/** Wrap the entire owner, not an app/driver or an individual WebDriver command. */
export function runnerCommand(args: string[], inherited: Environment = process.env): {
  program: string; args: string[]; env: Environment;
} {
  const env: Environment = { ...inherited, DESKTOP_TEST_WRAPPED: "1" };
  if (!LINUX) return { program: process.execPath, args, env };
  delete env.WAYLAND_DISPLAY;
  return {
    program: "dbus-run-session",
    args: ["--", "xvfb-run", "-a", process.execPath, ...args],
    env: { ...env, GDK_BACKEND: "x11", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" },
  };
}

/** Startup regression fixtures never admit a native app. Their test timeout must
 * terminate the entire wrapped runner, including dbus/Xvfb and fixture descendants,
 * then join its closed pipes before the test removes any inputs. This is not the
 * suite's app lifecycle: that remains exclusively with WDIO's Tauri service. */
export async function fixtureRunner(command: ReturnType<typeof runnerCommand>, signal: AbortSignal): Promise<{
  code: number | null; stdout: string; stderr: string;
}> {
  signal.throwIfAborted();
  const child = spawn(command.program, command.args, {
    cwd: resolve(import.meta.dirname, ".."), env: command.env,
    detached: !WINDOWS, stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  const failures: unknown[] = [];
  child.stdout.on("data", (chunk: Buffer) => { stdout += chunk; });
  child.stderr.on("data", (chunk: Buffer) => { stderr += chunk; });
  // Abort and spawn errors can precede close; neither releases fixture ownership.
  child.on("error", (error) => { failures.push(error); });
  const closed = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done) => {
    child.once("close", (code, signal) => done({ code, signal }));
  });
  let cancellation: Promise<void> | undefined;
  const cancel = () => {
    cancellation ??= stopTree(child).catch((error: unknown) => { failures.push(error); });
  };
  signal.addEventListener("abort", cancel, { once: true });
  if (signal.aborted) cancel();
  const outcome = await closed;
  signal.removeEventListener("abort", cancel);
  await cancellation;
  if (signal.aborted) failures.unshift(signal.reason);
  else if (outcome.signal) failures.push(new Error(`fixture runner terminated by ${outcome.signal}`));
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "fixture runner failed or could not be cancelled");
  return { code: outcome.code, stdout, stderr };
}

/** Stop a child started with its own process tree, and every descendant of it. */
async function stopTree(child: ChildProcess): Promise<void> {
  if (child.pid === undefined) return;
  if (!WINDOWS) {
    const group = -child.pid;
    // A group exists while any member does: the signal is followed until none is left,
    // so no descendant outlives the runner's fixture inputs.
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
    return;
  }
  // Windows has no POSIX process groups; its declared OS tool owns the same tree.
  const root = variable("SystemRoot");
  if (!root) throw new Error("SystemRoot is required to cancel a Windows fixture runner");
  const killer = spawn(join(root, "System32", "taskkill.exe"), ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  let failure: Error | undefined;
  killer.once("error", (error) => { failure = error; });
  const code = await new Promise<number | null>((done) => { killer.once("close", done); });
  if (failure) throw failure;
  if (code !== 0 && child.exitCode === null && child.signalCode === null) throw new Error(`taskkill exited ${code}`);
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
  if (WINDOWS) throw new Error("Start desktop E2E through pnpm e2e so its executable entry point is inherited");
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

export type LoopbackServer = { port: number; listening: () => boolean; close: () => Promise<void> };

/** Listen on a loopback TCP port (0: one the OS assigns) and hand each connection over. */
export function serveLoopback(onConnection: (socket: Socket) => void = (socket) => socket.end(), port = 0): Promise<LoopbackServer> {
  const server = createServer(onConnection);
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      if (!address || typeof address === "string") return fail(new Error("no loopback port was assigned"));
      done({
        port: address.port,
        listening: () => server.listening,
        close: () => server.listening
          ? new Promise<void>((closed, failed) => server.close((error) => error ? failed(error) : closed()))
          : Promise.resolve(),
      });
    });
  });
}

export type HttpServer = { url: string; close: () => Promise<void> };

/** Serve HTTP on a loopback port the OS assigns, as http://127.0.0.1:<port>. Closing also
 * ends kept-alive connections, so the port is released when `close` resolves. */
export function serveLoopbackHttp(handle: (request: IncomingMessage, response: ServerResponse) => void): Promise<HttpServer> {
  const server = createHttpServer(handle);
  const close = () => new Promise<void>((done, fail) => {
    server.closeAllConnections();
    if (!server.listening) return done();
    server.close((error) => error ? fail(error) : done());
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
    return state.status === 0 && !state.stdout.trim().startsWith("Z");
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

export function assertRunnerMachine(): void {
  if (WINDOWS && !(variable("GITHUB_ACTIONS") === "true"
    && variable("RUNNER_ENVIRONMENT") === "github-hosted" && variable("RUNNER_OS") === "Windows")) {
    throw new Error("Windows desktop E2E requires a disposable GitHub-hosted Windows runner; its default URL handler cannot be isolated per app instance");
  }
}

export function assertHarnessPlatform(): void {
  assertRunnerMachine();
  if (!LINUX) return;
  if (!process.env.DISPLAY || !process.env.DBUS_SESSION_BUS_ADDRESS || process.env.GDK_BACKEND !== "x11" || process.env.WAYLAND_DISPLAY) {
    throw new Error("Linux desktop E2E requires the whole runner inside dbus-run-session -- xvfb-run -a, GDK_BACKEND=x11 and no WAYLAND_DISPLAY");
  }
}

/** Stop the system browser that Windows' default URL handler started for the account
 * journey. It inherits the fixture's environment, so its profile lives in the fixture and
 * it would hold the fixture until the machine ends; the suite runs on Windows only on a
 * disposable runner (assertRunnerMachine), whose default browser is Edge. Unix openers are
 * fixture programs that have already exited. */
export async function releaseAccountBrowser(): Promise<void> {
  if (!WINDOWS) return;
  const root = variable("SystemRoot");
  if (!root) throw new Error("SystemRoot is required to stop the Windows account browser");
  const stopped = await runProgramAsync(join(root, "System32", "taskkill.exe") as NativePath, ["/IM", "msedge.exe", "/T", "/F"]);
  // 128: no such process, the browser already exited.
  if (stopped.status !== 0 && stopped.status !== 128) throw new Error(`taskkill exited ${stopped.status}: ${stopped.stderr}`);
}

/** Unix desktop openers are declared fixture executables performing the real HTTP GET;
 * the account server, not this program, records the exact displayed address reached. */
export function accountBrowserEnvironment(root: NativePath, env: Environment): Environment {
  if (WINDOWS) return env;
  const bin = makeDirectory(nativePath(root, "browser-bin"));
  const program = nativePath(bin, "open-url.mjs");
  writeText(program, `const response = await fetch(process.argv[2]);\nawait response.arrayBuffer();\nif (!response.ok) throw new Error(\`browser HTTP \${response.status}\`);\n`);
  // Paths are arguments, never interpreted as shell source.
  const opener = MACOS ? "open" : "xdg-open";
  writeFileSync(join(bin, opener), '#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$DESKTOP_TEST_BROWSER_PROGRAM" "$@"\n', { mode: 0o755 });
  return { ...env, PATH: `${bin}${delimiter}${env.PATH ?? ""}`, DESKTOP_TEST_NODE: process.execPath, DESKTOP_TEST_BROWSER_PROGRAM: program };
}
