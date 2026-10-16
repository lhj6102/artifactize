// Native runner configuration lives here; journeys contain no OS lifecycle branches.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { basename, delimiter, isAbsolute, join, resolve } from "node:path";
import type { Fixture } from "./fixture.ts";

export const executableSuffix = process.platform === "win32" ? ".exe" : "";

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

/** Wrap the entire owner, not an app/driver or an individual WebDriver command. */
export function runnerCommand(args: string[], inherited: NodeJS.ProcessEnv = process.env): {
  program: string; args: string[]; env: NodeJS.ProcessEnv;
} {
  const env: NodeJS.ProcessEnv = { ...inherited, DESKTOP_TEST_WRAPPED: "1" };
  if (process.platform !== "linux") return { program: process.execPath, args, env };
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
    detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"],
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
    cancellation ??= stopFixtureRunner(child).catch((error: unknown) => { failures.push(error); });
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

async function stopFixtureRunner(child: ChildProcess): Promise<void> {
  if (child.pid === undefined) return;
  if (process.platform !== "win32") {
    try { process.kill(-child.pid, "SIGKILL"); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
    return;
  }
  // Windows has no POSIX process groups; its declared OS tool owns the same tree.
  const root = process.env.SystemRoot;
  if (!root) throw new Error("SystemRoot is required to cancel a Windows fixture runner");
  const killer = spawn(join(root, "System32", "taskkill.exe"), ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  let failure: Error | undefined;
  killer.once("error", (error) => { failure = error; });
  const code = await new Promise<number | null>((done) => { killer.once("close", done); });
  if (failure) throw failure;
  if (code !== 0 && child.exitCode === null && child.signalCode === null) throw new Error(`taskkill exited ${code}`);
}

/** Native display/session inputs survive fixture auth/profile sanitization. */
export function harnessEnvironment(): Record<string, string> {
  if (process.platform !== "linux") return {};
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
  if (process.platform === "win32" && !(process.env.GITHUB_ACTIONS === "true"
    && process.env.RUNNER_ENVIRONMENT === "github-hosted" && process.env.RUNNER_OS === "Windows")) {
    throw new Error("Windows desktop E2E requires a disposable GitHub-hosted Windows runner; its default URL handler cannot be isolated per app instance");
  }
}

export function assertHarnessPlatform(): void {
  assertRunnerMachine();
  if (process.platform !== "linux") return;
  if (!process.env.DISPLAY || !process.env.DBUS_SESSION_BUS_ADDRESS || process.env.GDK_BACKEND !== "x11" || process.env.WAYLAND_DISPLAY) {
    throw new Error("Linux desktop E2E requires the whole runner inside dbus-run-session -- xvfb-run -a, GDK_BACKEND=x11 and no WAYLAND_DISPLAY");
  }
}

/** Unix desktop openers are declared fixture executables performing the real HTTP GET;
 * the account server, not this program, records the exact displayed address reached. */
export function accountBrowserEnvironment(fixture: Fixture, env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  if (process.platform === "win32") return env;
  const bin = join(fixture.root, "browser-bin");
  mkdirSync(bin, { recursive: true });
  const program = join(bin, "open-url.mjs");
  writeFileSync(program, `const response = await fetch(process.argv[2]);\nawait response.arrayBuffer();\nif (!response.ok) throw new Error(\`browser HTTP \${response.status}\`);\n`);
  // Paths are arguments, never interpreted as shell source.
  const opener = process.platform === "darwin" ? "open" : "xdg-open";
  writeFileSync(join(bin, opener), '#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$DESKTOP_TEST_BROWSER_PROGRAM" "$@"\n', { mode: 0o755 });
  return { ...env, PATH: `${bin}${delimiter}${env.PATH ?? ""}`, DESKTOP_TEST_NODE: process.execPath, DESKTOP_TEST_BROWSER_PROGRAM: program };
}

/** pnpm scripts inherit their real CLI entry point. Use it rather than Windows'
 * .cmd shim, which Node cannot spawn without a shell. No command string is parsed. */
export function packageCommand(args: string[]): { program: string; args: string[] } {
  const entry = process.env.npm_execpath;
  if (entry && isAbsolute(entry)) {
    const name = basename(entry).toLowerCase();
    if (/^pnpm\.(?:c|m)?js$/.test(name)) return { program: process.execPath, args: [entry, ...args] };
    if (name === "pnpm" || name === "pnpm-native" || name.endsWith(".exe")) return { program: entry, args };
  }
  if (process.platform === "win32") throw new Error("Start desktop E2E through pnpm e2e so its executable entry point is inherited");
  return { program: "pnpm", args };
}
