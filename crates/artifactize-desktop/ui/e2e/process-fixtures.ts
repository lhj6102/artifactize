// Read-only lifetime observations for tools executed by the real application.
// There are no PID identities, process scans, signals or app lifecycle controls here.
import type { Fixture } from "./fixture.ts";
import { nativePath, readLoopback, readTextIfExists } from "./platform.ts";

/** A protocol watchdog, never a delay used to infer that a tool stopped. */
const CONNECTION_TIMEOUT = 5000;

type Endpoint = { port: number; token: string };

/** The captured endpoint is served by the actual tool process. A later process using
 * the same ephemeral port cannot prove liveness without the fixture's unique token. */
export function slowTool(fixture: Fixture): (() => Promise<boolean>) | null {
  const path = nativePath(fixture.root, "slow.endpoint");
  const text = readTextIfExists(path);
  if (text === null) return null;
  const saved: unknown = JSON.parse(text);
  if (typeof saved !== "object" || saved === null || !("port" in saved) || !("token" in saved)
    || !Number.isInteger(saved.port) || (saved.port as number) < 1 || (saved.port as number) > 65535 || typeof saved.token !== "string" || !saved.token) {
    throw new Error(`invalid tool endpoint ${path}`);
  }
  const endpoint = saved as Endpoint;
  // Refusal/reset is an actual failed connection, not an elapsed-time inference.
  return async () => (await readLoopback(endpoint.port, CONNECTION_TIMEOUT)) === endpoint.token;
}
