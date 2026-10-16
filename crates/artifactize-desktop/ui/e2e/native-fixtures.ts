// Portable domain commands. The runner owns the app and every native service;
// fixtures declare only programs that the real product executes.
import { fileURLToPath } from "node:url";

const command = process.execPath;
const script = fileURLToPath(new URL("./domain-fixtures/command.mjs", import.meta.url));

export type Operation = "pass" | "fail" | "read" | "launch" | "slow" | "hold";

/** JSON strings/arrays are also TOML basic strings/arrays, including Windows paths. */
export function commandFields(operation: Operation, ...args: string[]): string {
  return `command = ${JSON.stringify(command)}, args = ${JSON.stringify([script, operation, ...args])}`;
}

export function runtimeProfile(operation: Operation = "pass", timeoutMs?: number): string {
  return `{ kind = "runtime", ${commandFields(operation)}${timeoutMs === undefined ? "" : `, timeout_ms = ${timeoutMs}`} }`;
}
