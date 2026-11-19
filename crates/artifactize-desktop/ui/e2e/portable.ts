// Portable helpers of the desktop E2E suite: native paths built with node:path, files, loopback
// servers and SQLite fixtures. They behave the same on every OS; what differs between operating
// systems stays in platform.ts (E2E-PLATFORM).
import {
  appendFileSync, copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync,
} from "node:fs";
import { createServer as createHttpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createConnection, createServer, type Socket } from "node:net";
import { basename, dirname, isAbsolute, join, normalize, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";

declare const native: unique symbol;
export type { Socket as LoopbackSocket } from "node:net";

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

/** A path written into a file, a declaration or an argument template, or compared with
 * what the product shows: segments joined by `/` on every OS. */
export function textPath(...segments: string[]): string {
  return segments.join("/");
}

export function exists(path: NativePath): boolean {
  return existsSync(path);
}

/** Create a directory and its parents; `private` keeps it to the current user. */
export function makeDirectory(path: NativePath, { private: owner = false } = {}): NativePath {
  mkdirSync(path, { recursive: true, ...(owner ? { mode: 0o700 } : {}) });
  return path;
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

/** Save what the window shows as a PNG file. */
export async function saveScreenshot(browser: WebdriverIO.Browser, path: NativePath): Promise<void> {
  writeFileSync(path, Buffer.from(await browser.takeScreenshot(), "base64"));
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

export type Database = DatabaseSync;

/** Change a file-backed SQLite database whose handle never outlives the call. */
export function updateDatabase(path: NativePath, change: (database: Database) => void): void {
  const database = new DatabaseSync(path);
  try {
    database.exec("PRAGMA busy_timeout=5000");
    change(database);
  } finally { database.close(); }
}

/** Loopback ports that stand for once-per-machine resources, outside every OS's ephemeral
 * range: while a run holds one, a second run on this machine cannot claim it, and the OS frees
 * it when the holder's process ends, even by a crash. */
const MACHINE_RESOURCES = { "auth-callback": 3455, clipboard: 3456, "foreground-window": 3457 } as const;

/** Claim a once-per-machine resource for this run (E2E-ISOLATED); the result releases it. A
 * resource another run holds is an explicit failure, not a wait. */
export async function claimMachineResource(name: keyof typeof MACHINE_RESOURCES): Promise<() => Promise<void>> {
  try {
    return (await serveLoopback(undefined, MACHINE_RESOURCES[name])).close;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EADDRINUSE") throw error;
    throw new Error(`another E2E run on this machine holds the ${name}; one app instance uses it at a time`, { cause: error });
  }
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

/** Original bytes, for checksum-based upstream provenance rather than text normalization. */
export function readBytes(path: NativePath): Uint8Array { return readFileSync(path); }

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

/** Failure-only native/backend diagnostics from this batch's service-owned log files. */
export function serviceFailureLogs(directory: NativePath): string[] {
  // Keep startup and crash causes visible in CI without replaying entire backend logs.
  const tailCharacters = 16 * 1024;
  return sourceTree(directory).filter((item) => item.kind === "file" && item.name.endsWith(".log"))
    .map((item) => `--- service log ${item.name} ---\n${readText(item.path).slice(-tailCharacters)}`);
}

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
