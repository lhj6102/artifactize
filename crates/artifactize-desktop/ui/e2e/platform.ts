// Native runner configuration lives here; journeys contain no OS lifecycle branches.
import { mkdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { basename, delimiter, isAbsolute, join } from "node:path";
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

export function assertHarnessPlatform(): void {
  if (process.platform !== "linux") return;
  if (!process.env.DISPLAY || !process.env.DBUS_SESSION_BUS_ADDRESS || process.env.GDK_BACKEND !== "x11" || process.env.WAYLAND_DISPLAY) {
    throw new Error("Linux desktop E2E requires the whole runner inside dbus-run-session -- xvfb-run -a, GDK_BACKEND=x11 and no WAYLAND_DISPLAY");
  }
}

/** Linux CI has no desktop browser. This declared opener performs the real HTTP GET;
 * the account server, not this program, records the exact displayed address reached. */
export function accountBrowserEnvironment(fixture: Fixture, env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  if (process.platform !== "linux") return env;
  const bin = join(fixture.root, "browser-bin");
  mkdirSync(bin, { recursive: true });
  const program = join(bin, "open-url.mjs");
  writeFileSync(program, `const response = await fetch(process.argv[2]);\nawait response.arrayBuffer();\nif (!response.ok) throw new Error(\`browser HTTP \${response.status}\`);\n`);
  // Paths are arguments, never interpreted as shell source.
  writeFileSync(join(bin, "xdg-open"), '#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$DESKTOP_TEST_BROWSER_PROGRAM" "$@"\n', { mode: 0o755 });
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
