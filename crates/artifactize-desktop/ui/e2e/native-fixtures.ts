// Portable domain commands. The runner owns the app and every native service;
// fixtures declare only programs that the real product executes.
import { nodeProgram, portablePath, suitePath } from "./platform.ts";

/** The program and script every declared fixture command runs, as declarations spell paths. */
export const command = portablePath(nodeProgram);
export const script = portablePath(suitePath("domain-fixtures", "command.ts"));

export type Operation = "pass" | "fail" | "read" | "launch" | "slow" | "hold";

/** JSON strings/arrays are also TOML basic strings/arrays, including Windows paths. */
export function commandFields(operation: Operation, ...args: string[]): string {
  return `command = ${JSON.stringify(command)}, args = ${JSON.stringify([script, operation, ...args])}`;
}

export function runtimeProfile(operation: Operation = "pass", timeoutMs?: number): string {
  return `{ kind = "runtime", ${commandFields(operation)}${timeoutMs === undefined ? "" : `, timeout_ms = ${timeoutMs}`} }`;
}
