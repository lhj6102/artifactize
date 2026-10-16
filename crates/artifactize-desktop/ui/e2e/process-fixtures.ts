// Read-only lifetime observations for tools executed by the real application.
// There are no PID identities, process scans, signals or app lifecycle controls here.
import { readFileSync } from "node:fs";
import { createConnection } from "node:net";
import { join } from "node:path";
import type { Fixture } from "./fixture.ts";

/** A protocol watchdog, never a delay used to infer that a tool stopped. */
const CONNECTION_TIMEOUT = 5000;

type Endpoint = { port: number; token: string };

/** The captured endpoint is served by the actual tool process. A later process using
 * the same ephemeral port cannot prove liveness without the fixture's unique token. */
export function slowTool(fixture: Fixture): (() => Promise<boolean>) | null {
  const path = join(fixture.root, "slow.endpoint");
  let endpoint: Endpoint;
  try {
    const saved: unknown = JSON.parse(readFileSync(path, "utf8"));
    if (typeof saved !== "object" || saved === null || !("port" in saved) || !("token" in saved)
      || !Number.isInteger(saved.port) || (saved.port as number) < 1 || (saved.port as number) > 65535 || typeof saved.token !== "string" || !saved.token) {
      throw new Error(`invalid tool endpoint ${path}`);
    }
    endpoint = saved as Endpoint;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
  return () => alive(endpoint);
}

function alive(endpoint: Endpoint): Promise<boolean> {
  return new Promise((done, fail) => {
    const socket = createConnection({ host: "127.0.0.1", port: endpoint.port });
    let output = "";
    socket.setEncoding("utf8");
    socket.setTimeout(CONNECTION_TIMEOUT, () => socket.destroy(new Error("tool lifetime observation timed out")));
    socket.on("data", (chunk: string) => { output += chunk; });
    socket.once("end", () => done(output === endpoint.token));
    socket.once("error", (error: NodeJS.ErrnoException) => {
      // Refusal/reset is an actual failed connection, not an elapsed-time inference.
      if (["ECONNREFUSED", "ECONNRESET"].includes(error.code ?? "")) done(false);
      else fail(error);
    });
    socket.once("close", () => socket.destroy());
  });
}
