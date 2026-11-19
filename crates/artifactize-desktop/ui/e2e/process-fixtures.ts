// Read-only lifetime observations for tools executed by the real application.
// There are no PID identities, process scans, signals or app lifecycle controls here.
import type { Fixture } from "./fixture.ts";
import { processRunning } from "./platform.ts";
import { closeDirectory, exists, listFiles, nameOf, nativePath, readLoopback, readTextIfExists } from "./portable.ts";
import { observe } from "./domain-fixtures/observe.ts";

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

/** Stop accepting tool registrations before releasing every already registered tool. */
export function releaseToolGates(fixture: Fixture): void {
  closeDirectory(nativePath(fixture.root, "gated-tools"), nativePath(fixture.root, "closed-gated-tools"));
}

/** Each empty, exclusive PID file is the admission: even if teardown renames the
 * directory between opening and closing that file, the record moves with it. */
export async function joinGatedTools(fixture: Fixture): Promise<void> {
  await observe(() => ["gated-tools", "closed-gated-tools"].every((name) => {
    const directory = nativePath(fixture.root, name);
    return !exists(directory) || listFiles(directory).every((file) => {
      const match = /^(\d+)-[a-f0-9-]+\.pid$/.exec(nameOf(file));
      const pid = Number(match?.[1]);
      if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error(`invalid gated tool identity: ${file}`);
      return !processRunning(pid);
    });
  }), () => `gated tools still own fixture ${fixture.root}; preserve inputs`);
}
