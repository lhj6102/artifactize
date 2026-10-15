// Linux WebKitGTK account fixtures own the provider, browser callback and app's network
// together. Isolate only these tests, never Cargo builds or the person's fixed login port.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readlinkSync, writeFileSync } from "node:fs";
import { it } from "node:test";
import { fileURLToPath } from "node:url";
import { Owned } from "./owned.ts";
import { moduleURL, NativeProcess, nativeFixture } from "./native-fixtures.ts";

const PARENT = "DESKTOP_TEST_PARENT_NETWORK";
// Ubuntu Noble restricts direct userns creation but ships a policy for this LXC helper.
// Select installed OS support once, never retry a failed setup through another interface.
const userNamespace = existsSync("/usr/bin/lxc-usernsexec") ? "/usr/bin/lxc-usernsexec" : "/usr/bin/unshare";
export const networkIdentity = () => readlinkSync("/proc/self/ns/net");

/** This namespace disappears with its processes. Map only the calling non-root user,
 * never host root or subordinate ranges; setup/security failures remain test failures. */
export function networkProcess(program: string, args: string[], inherited = process.env): Owned {
  if (process.platform !== "linux") throw new Error("private account networks require the Linux WebKitGTK fixture layer");
  const uid = process.getuid!();
  const gid = process.getgid!();
  if (uid === 0 || gid === 0) throw new Error("private account networks must not map host root");
  const env: NodeJS.ProcessEnv = { ...inherited, [PARENT]: networkIdentity() };
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

// Native observations for the namespace seam tests. The cases never query procfs,
// look up host identities/environment, or construct their own namespace processes.
function hostIdentity() {
  return { network: networkIdentity(), user: readlinkSync("/proc/self/ns/user"),
    maps: ["uid_map", "gid_map"].map((name) => readFileSync(`/proc/self/${name}`, "utf8")),
    uid: process.getuid!(), gid: process.getgid!() };
}

type NetworkReady = { network: string; user: string; id: string; uid: number; gid: number; uidMap: number[]; gidMap: number[] };
const callbackProbe = `
import { createServer } from "node:http";
import { readFileSync, readlinkSync } from "node:fs";
import { createInterface } from "node:readline";
const id = process.argv[1];
const server = createServer((request, response) => response.end(id + ":" + request.url));
server.listen(1455, "127.0.0.1", () => console.log(JSON.stringify({
  phase: "ready", network: readlinkSync("/proc/self/ns/net"), user: readlinkSync("/proc/self/ns/user"), id,
  uid: process.getuid(), gid: process.getgid(),
  uidMap: readFileSync("/proc/self/uid_map", "utf8").trim().split(/\\s+/).map(Number),
  gidMap: readFileSync("/proc/self/gid_map", "utf8").trim().split(/\\s+/).map(Number),
})));
const lines = createInterface({ input: process.stdin });
lines.once("line", async () => {
  const response = await fetch("http://127.0.0.1:1455/auth/callback");
  console.log(JSON.stringify({ phase: "returned", body: await response.text() }));
  lines.close();
  process.stdin.pause();
  server.close();
});
process.stdin.once("end", () => server.close());`;

export async function callbackNetworks() {
  return nativeFixture((scratch) => {
  const host = hostIdentity();
  const owners = ["first", "second"].map((id) => {
    const owner = new NativeProcess(networkProcess(process.execPath, ["--input-type=module", "-e", callbackProbe, id], scratch.env()), {});
    scratch.processes.push(owner);
    return owner;
  });
  return { host, owners,
    ready: () => Promise.all(owners.map(async (owner) => JSON.parse(await owner.line()) as NetworkReady)),
    callback() { for (const owner of owners) owner.send("callback\n"); },
    hostNow: hostIdentity, stop: () => scratch.dispose(),
  };
  });
}

export async function privateNetworkFailure() {
  return nativeFixture((scratch) => {
  const file = scratch.path("nested.test.ts");
  writeFileSync(file, `import { it } from "node:test";
import { networkTests } from ${JSON.stringify(moduleURL("./network.ts"))};
networkTests(import.meta.url, () => {
  it("the original private case", () => { throw new Error("the original private failure"); });
});\n`);
  const owner = new NativeProcess("", scratch.env({ [PARENT]: undefined }), [], false,
    { command: process.execPath, args: ["--test", file] });
  scratch.processes.push(owner);
  return { owner, stop: () => scratch.dispose() };
  });
}
