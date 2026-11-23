// Portable helpers of the desktop E2E suite: native paths built with node:path, files,
// programs, loopback servers and SQLite fixtures. They behave the same on every OS; what
// differs between operating systems stays in platform.ts (E2E-PLATFORM).
import { spawn, spawnSync } from "node:child_process";
import {
  appendFileSync, chmodSync, copyFileSync, existsSync, linkSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync,
  renameSync, rmSync, writeFileSync,
} from "node:fs";
import { createServer as createHttpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, normalize, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";

declare const native: unique symbol;

/** A path in this OS's own form, for file system and process calls only. */
export type NativePath = string & { readonly [native]: true };

export function nativePath(first: string, ...rest: string[]): NativePath {
  return join(first, ...rest) as NativePath;
}

/** An absolute native path from segments, resolved against the current directory. */
export function absolutePath(first: string, ...rest: string[]): NativePath {
  return resolve(first, ...rest) as NativePath;
}

/** An absolute path from a variable, an argument or serialized data, in its native form. */
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

/** A path written into a declaration or compared with what the app shows: `/` on every OS. */
export function textPath(...segments: string[]): string {
  return segments.join("/");
}

/** A file of the E2E suite itself. */
export function suitePath(...segments: string[]): NativePath {
  return join(import.meta.dirname, ...segments) as NativePath;
}

/** The folder the suite's scratch lives under: the product's declared scratch folder when the
 * suite runs as an eval, otherwise the OS temporary folder. */
export function scratchDirectory(): NativePath {
  return (process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir()) as NativePath;
}

export function exists(path: NativePath): boolean {
  return existsSync(path);
}

/** Create a directory and its parents; `private` keeps it to the current user. */
export function makeDirectory(path: NativePath, { private: owner = false } = {}): NativePath {
  mkdirSync(path, { recursive: true, ...(owner ? { mode: 0o700 } : {}) });
  return path;
}

/** A new, uniquely named directory under `parent`, as its canonical path: links resolved,
 * Windows short names and macOS `/var` aliases expanded, as the app canonicalizes paths. */
export function newDirectory(parent: NativePath, prefix: string): NativePath {
  makeDirectory(parent);
  return realpathSync.native(mkdtempSync(join(parent, prefix))) as NativePath;
}

/** Remove a directory tree, retrying while the OS still holds it: a web engine's helper
 * processes and the tools an app started let go of their files a moment after it ends. */
export function removeTree(path: NativePath): void {
  rmSync(path, { recursive: true, force: true, maxRetries: 15, retryDelay: 200 });
}

/** Hold the machine-wide lock `name` while `body` runs: another runner on this machine, in
 * another suite run, waits for it. The lock is a loopback port of its own, listened on, which
 * the system frees when its holder goes, however it ends. */
export async function withMachineLock<T>(name: string, body: () => Promise<T>): Promise<T> {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) % 2000;
  const port = 47100 + hash;
  const deadline = Date.now() + 60 * 60 * 1000;
  const server = createServer();
  for (;;) {
    const taken = await new Promise<boolean>((done, fail) => {
      server.once("error", (error: NodeJS.ErrnoException) => (error.code === "EADDRINUSE" ? done(false) : fail(error)));
      server.listen({ port, host: "127.0.0.1", exclusive: true }, () => done(true));
    });
    if (taken) break;
    if (Date.now() > deadline) throw new Error(`the machine lock ${name} (127.0.0.1:${port}) was held for an hour`);
    await new Promise((done) => setTimeout(done, 500));
  }
  try { return await body(); }
  finally { await new Promise((done) => server.close(done)); }
}

/** Entries of `directory` that are regular files, sorted by name. */
export function listFiles(directory: NativePath): NativePath[] {
  return readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isFile())
    .map((entry) => join(directory, entry.name) as NativePath).sort();
}

/** Text files are UTF-8 and keep `\n` line endings on every OS. */
export function readText(path: NativePath): string {
  return readFileSync(path, "utf8");
}

/** A text file's content, or `null` when it does not exist. */
export function readTextIfExists(path: NativePath): string | null {
  try { return readText(path); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

/** Write a text file; `private` keeps it to the user. */
export function writeText(path: NativePath, text: string, { private: owner = false } = {}): void {
  writeFileSync(path, text, owner ? { mode: 0o600 } : {});
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

export function readJson(path: NativePath): unknown {
  return JSON.parse(readText(path));
}

export function writeJson(path: NativePath, value: unknown, options: { private?: boolean } = {}): void {
  writeText(path, JSON.stringify(value), options);
}

export function copyFile(from: NativePath, to: NativePath): void {
  copyFileSync(from, to);
}

/** The app as a link in `directory`, so it runs from where an install channel puts it while the
 * build stays one file; a copy where a link cannot cross volumes. */
export function placeApp(app: NativePath, directory: NativePath): NativePath {
  const placed = nativePath(makeDirectory(directory), nameOf(app));
  try { linkSync(app, placed); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EXDEV") throw error;
    copyFileSync(app, placed);
    chmodSync(placed, 0o755);
  }
  return placed;
}

// ---------------------------------------------------------------------------------
// Programs

export type Completed = { status: number; stdout: string; stderr: string };
type RunOptions = { env?: NodeJS.ProcessEnv; cwd?: NativePath; input?: string; timeout?: number };

/** Run a program with an argument vector (never a shell string) until it exits. A program
 * that cannot start is an error; its exit status is the caller's to judge. */
export function runProgram(program: string, args: readonly string[], options: RunOptions = {}): Completed {
  const result = spawnSync(program, args, { encoding: "utf8", windowsHide: true, timeout: 120000, ...options });
  if (result.error) throw result.error;
  return { status: result.status ?? -1, stdout: result.stdout, stderr: result.stderr };
}

/** As `runProgram`, but an exit status other than 0 is an error. */
export function runProgramOk(program: string, args: readonly string[], options: RunOptions = {}): string {
  const result = runProgram(program, args, options);
  if (result.status !== 0) throw new Error(`${program} ${args.join(" ")} exited ${result.status}: ${result.stderr}${result.stdout}`);
  return result.stdout;
}

/** As `runProgram`, without blocking this process, so a server it serves can answer. */
export function runProgramAsync(program: string, args: readonly string[], options: RunOptions = {}): Promise<Completed> {
  return new Promise((done, fail) => {
    const child = spawn(program, args, { env: options.env, cwd: options.cwd, windowsHide: true, timeout: options.timeout ?? 120000 });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => { stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => { stderr += chunk; });
    child.stdin.end(options.input ?? "");
    child.once("error", fail);
    child.once("close", (code) => done({ status: code ?? -1, stdout, stderr }));
  });
}

/** Run a program with this terminal's input and output until it exits; its exit status. */
export function runVisible(program: string, args: readonly string[], env: NodeJS.ProcessEnv, cwd: NativePath): Promise<number> {
  return new Promise((done, fail) => {
    const child = spawn(program, args, { env, cwd, stdio: "inherit" });
    child.once("error", fail);
    child.once("close", (code) => done(code ?? 1));
  });
}

// ---------------------------------------------------------------------------------
// Loopback

export type HttpServer = { url: string; port: number; close: () => Promise<void> };

/** Serve HTTP on a loopback port the OS assigns, as http://127.0.0.1:<port>. Closing ends
 * kept-alive connections too, so the port is free once `close` resolves. */
export function serveLoopbackHttp(handle: (request: IncomingMessage, response: ServerResponse) => void, port = 0): Promise<HttpServer> {
  const server = createHttpServer(handle);
  let closing: Promise<void> | undefined;
  const close = () => closing ??= new Promise<void>((done, fail) => {
    server.close((error) => error ? fail(error) : done());
    server.closeAllConnections();
  });
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      if (!address || typeof address === "string") return fail(new Error("no loopback port was assigned"));
      done({ url: `http://127.0.0.1:${address.port}`, port: address.port, close });
    });
  });
}

/** Listen on the loopback port `port`: the server, or `undefined` when the port is taken or
 * not this user's to listen on. */
function listenLoopback(port: number): Promise<Server | undefined> {
  const server = createServer();
  return new Promise((done, fail) => {
    server.once("error", (error: NodeJS.ErrnoException) => (error.code === "EADDRINUSE" || error.code === "EACCES" ? done(undefined) : fail(error)));
    server.listen({ port, host: "127.0.0.1", exclusive: true }, () => done(server));
  });
}

const closed = (server: Server) => new Promise<void>((done) => server.close(() => done()));

/** Reserve each embedded WebDriver port with a companion listener until its owner finishes. */
export async function embeddedPorts(count: number): Promise<{ ports: number[]; close: () => Promise<void> }> {
  const ports: number[] = [];
  const claims: Server[] = [];
  const close = async () => { await Promise.all(claims.splice(0).map(closed)); };
  try {
    for (let attempt = 0; ports.length < count; attempt += 1) {
      if (attempt >= 200 * count) throw new Error("no free pair of loopback ports was found between 20000 and 32767");
      const port = 20000 + 2 * Math.floor(Math.random() * 6384);
      const claim = await listenLoopback(port + 1);
      if (!claim) continue;
      claims.push(claim);
      const free = await listenLoopback(port);
      if (!free) {
        await closed(claim);
        claims.pop();
        continue;
      }
      await closed(free);
      claim.unref();
      ports.push(port);
    }
    return { ports, close };
  } catch (error) {
    await close();
    throw error;
  }
}

// ---------------------------------------------------------------------------------
// Files read whole

/** A file's original bytes, for checksums and images. */
export function readBytes(path: NativePath): Uint8Array { return readFileSync(path); }

export function writeBytes(path: NativePath, bytes: Uint8Array): void { writeFileSync(path, bytes); }

/** Every non-directory entry of a source tree, including links and special files. */
export function sourceTree(root: NativePath): { path: NativePath; name: string; kind: "file" | "other" }[] {
  const visit = (directory: NativePath, prefix: string): { path: NativePath; name: string; kind: "file" | "other" }[] =>
    readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
      const path = nativePath(directory, entry.name);
      const name = prefix ? textPath(prefix, entry.name) : entry.name;
      return entry.isDirectory() ? visit(path, name) : [{ path, name, kind: entry.isFile() ? "file" as const : "other" as const }];
    });
  return visit(root, "").sort((left, right) => left.name.localeCompare(right.name));
}

/** Change a file-backed SQLite database whose handle never outlives the call. */
export function updateDatabase(path: NativePath, change: (database: DatabaseSync) => void): void {
  const database = new DatabaseSync(path);
  try {
    database.exec("PRAGMA busy_timeout=5000");
    change(database);
  } finally { database.close(); }
}

/** Call the app window's own Tauri API (the frame's close, focus, title or visibility), which the
 * page cannot reach; what it answers. */
export async function callWindow(browser: WebdriverIO.Browser, method: "setFocus" | "close" | "title" | "isVisible"): Promise<unknown> {
  const answer = await browser.executeAsync((name: string, done: (answer: { value?: unknown; error?: string }) => void) => {
    type Api = { __TAURI__: { window: { getCurrentWindow: () => Record<string, () => Promise<unknown>> } } };
    (window as unknown as Api).__TAURI__.window.getCurrentWindow()[name]().then((value) => done({ value }), (error: unknown) => done({ error: String(error) }));
  }, method);
  if (answer.error !== undefined) throw new Error(`the window's ${method} failed: ${answer.error}`);
  return answer.value;
}
