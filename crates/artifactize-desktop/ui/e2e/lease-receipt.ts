// A per-candidate receipt outlives its directory, so directory absence cannot acknowledge
// a detached cleanup's final output and process exit. This is the same Linux Group boundary.
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { groupAlive, groupIdentity, type Group } from "./owned.ts";

/** Watchdogs need time to join their native groups; expiry preserves inputs and diagnostics. */
export const CLEANUP_TIMEOUT = 20000;
export const RECEIPT_SUFFIX = ".lease.json";
const receipt = (root: string) => root + RECEIPT_SUFFIX;

/** Fixture environments may remove ARTIFACTIZE_OUTPUT_DIR and nest TMPDIR in a candidate.
 * Lift logs outside every enclosing owned candidate, not merely outside the newest child. */
export function diagnosticsDirectory(prefix: string, env = process.env): string {
  const preferred = resolve(env.ARTIFACTIZE_OUTPUT_DIR ?? tmpdir());
  mkdirSync(preferred, { recursive: true });
  let output = realpathSync(preferred);
  for (let ancestor = output; ; ancestor = dirname(ancestor)) {
    if (existsSync(join(ancestor, "groups")) && existsSync(join(ancestor, "tmp"))) output = dirname(ancestor);
    if (dirname(ancestor) === ancestor) break;
  }
  return mkdtempSync(join(output, prefix));
}

export function recordLease(root: string, pid: number, diagnostics: string): void {
  const owner = { ...groupIdentity(pid), diagnostics };
  // Keep the recovery pointer with the durable logs too, including after receipt release.
  writeFileSync(join(diagnostics, "owner.json"), JSON.stringify({ root, ...owner }) + "\n");
  writeFileSync(receipt(root), JSON.stringify(owner) + "\n", { flag: "wx" });
}

/** Called only after the actual local close or a detached lifetime's observed termination. */
export function forgetLease(root: string): void {
  rmSync(receipt(root), { force: true });
}

export async function joinLease(root: string): Promise<void> {
  let value: unknown;
  try { value = JSON.parse(readFileSync(receipt(root), "utf8")); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
    throw error;
  }
  if (!value || typeof value !== "object") throw new Error(`invalid cleanup receipt: ${root}`);
  const { group, start_time, diagnostics } = value as Partial<Group> & { diagnostics?: string };
  if (typeof group !== "number" || !Number.isSafeInteger(group) || group <= 0 ||
      typeof start_time !== "number" || !Number.isSafeInteger(start_time) || start_time < 0 || typeof diagnostics !== "string") {
    throw new Error(`invalid cleanup receipt: ${root}`);
  }
  const deadline = Date.now() + CLEANUP_TIMEOUT;
  while (groupAlive(group, start_time)) {
    if (Date.now() >= deadline) throw new Error(`detached cleanup did not exit; preserve ${root}; diagnostics: ${diagnostics}`);
    await new Promise((done) => setTimeout(done, 50));
  }
  // Exit joins all file-descriptor users. Only now is the final report complete, even
  // when the cleanup removed its own candidate before writing the report.
  forgetLease(root);
  const result = JSON.parse(readFileSync(join(diagnostics, "outcome.json"), "utf8")) as { code?: number | null; signal?: string | null; disarmed?: boolean };
  if (!(result.code === 0 && result.signal === null || result.disarmed === true && result.signal === "SIGKILL")) {
    throw new Error(`detached candidate cleanup failed: ${JSON.stringify(result)}; diagnostics: ${diagnostics}`);
  }
}
