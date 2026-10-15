// Linux WebKitGTK account fixtures own the provider, browser callback and app's network
// together. Isolate only these tests, never Cargo builds or the person's fixed login port.
import assert from "node:assert/strict";
import { existsSync, readlinkSync } from "node:fs";
import { it } from "node:test";
import { fileURLToPath } from "node:url";
import { Owned } from "./owned.ts";

const PARENT = "DESKTOP_TEST_PARENT_NETWORK";
// Ubuntu Noble restricts direct userns creation but ships a policy for this LXC helper.
// Select installed OS support once, never retry a failed setup through another interface.
const userNamespace = existsSync("/usr/bin/lxc-usernsexec") ? "/usr/bin/lxc-usernsexec" : "/usr/bin/unshare";
export const networkIdentity = () => readlinkSync("/proc/self/ns/net");

/** This namespace disappears with its processes. Map only the calling non-root user,
 * never host root or subordinate ranges; setup/security failures remain test failures. */
export function networkProcess(program: string, args: string[]): Owned {
  if (process.platform !== "linux") throw new Error("private account networks require the Linux WebKitGTK fixture layer");
  const uid = process.getuid!();
  const gid = process.getgid!();
  if (uid === 0 || gid === 0) throw new Error("private account networks must not map host root");
  const env: NodeJS.ProcessEnv = { ...process.env, [PARENT]: networkIdentity() };
  // This is a new explicit test runner, not the parent's Node test-file subprocess.
  delete env.NODE_TEST_CONTEXT;
  const setup = userNamespace === "/usr/bin/lxc-usernsexec"
    ? ["-m", `u:0:${uid}:1`, "-m", `g:0:${gid}:1`, "--", "/usr/bin/unshare", "--net"]
    : ["--user", "--map-root-user", "--net"];
  return Owned.spawn(userNamespace, [...setup, "--", "sh", "-ec",
    'ip link set lo up; exec "$@"', "desktop-account-network", program, ...args], {
    env, stdio: ["pipe", "pipe", "pipe"],
  });
}

/** Both the normal suite and direct `node --test accounts.test.ts` enter this same
 * boundary. Only the child registers the real cases, and only after actual isolation. */
export function networkTests(url: string, define: () => void): void {
  const parent = process.env[PARENT];
  if (parent) {
    assert.notEqual(networkIdentity(), parent, "the account test must own a different network namespace, not just inherit a marker");
    define();
    return;
  }
  it("runs every account journey in its owned Linux network", async () => {
    const owner = networkProcess(process.execPath, ["--test", fileURLToPath(url)]);
    // Forward the real cases and their diagnostics instead of replacing them with a
    // synthetic summary. The outer case also propagates any nonzero child exit.
    owner.child.stdout!.pipe(process.stdout, { end: false });
    owner.child.stderr!.pipe(process.stderr, { end: false });
    const failures: unknown[] = [];
    try {
      const result = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
        owner.child.once("error", fail);
        owner.child.once("close", (code, signal) => done({ code, signal }));
      });
      assert.equal(result.code, 0, `account journeys failed: ${result.code ?? result.signal}`);
    } catch (error) { failures.push(error); }
    try { await owner.stop(); } catch (error) { failures.push(error); }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures, "account journeys and namespace shutdown failed");
  });
}
