// Exercise the Linux OS seam without a WebView: independent fixtures can own the real
// fixed callback port concurrently, and a child test failure remains a test failure.
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { it } from "node:test";
import { Owned } from "./owned.ts";
import { networkIdentity, networkProcess } from "./network.ts";

const probe = `
import { createServer } from "node:http";
import { readlinkSync } from "node:fs";
import { createInterface } from "node:readline";
const id = process.argv[1];
const server = createServer((request, response) => {
  response.end(id + ":" + request.url);
});
server.listen(1455, "127.0.0.1", () => console.log(JSON.stringify({
  phase: "ready", network: readlinkSync("/proc/self/ns/net"), id,
})));
const lines = createInterface({ input: process.stdin });
lines.once("line", async () => {
  const response = await fetch("http://127.0.0.1:1455/auth/callback");
  console.log(JSON.stringify({ phase: "returned", body: await response.text() }));
  lines.close();
  process.stdin.pause();
  server.close();
});
process.stdin.once("end", () => server.close());
`;

function observe(owner: Owned) {
  let output = "";
  let errors = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const ended = new Promise<number | null>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", (code) => done(code));
  });
  // Attach immediately, including a fast unshare/IP setup failure before readiness.
  void ended.catch(() => {});
  return { ended, output: () => output, errors: () => errors };
}

it("owns two distinct callback networks on port 1455 at once, without changing the host", async () => {
  const host = networkIdentity();
  const owners = ["first", "second"].map((id) => networkProcess(process.execPath, ["--input-type=module", "-e", probe, id]));
  const observed = owners.map(observe);
  try {
    const ready = await Promise.all(owners.map((owner, index) => new Promise<{ network: string; id: string }>((done, fail) => {
      const lines = createInterface({ input: owner.child.stdout! });
      const deadline = setTimeout(() => finish(new Error(`callback network did not become ready: ${observed[index].errors()}`)), 5000);
      function finish(error?: Error, value?: { network: string; id: string }) {
        clearTimeout(deadline);
        lines.close();
        if (error) fail(error); else done(value!);
      }
      lines.once("line", (line) => {
        try { finish(undefined, JSON.parse(line) as { network: string; id: string }); }
        catch (error) { finish(error instanceof Error ? error : new Error(String(error))); }
      });
      void observed[index].ended.then((code) => finish(new Error(`callback network exited ${code}: ${observed[index].errors()}`)), fail);
    })));
    assert.notEqual(ready[0].network, host);
    assert.notEqual(ready[1].network, host);
    assert.notEqual(ready[0].network, ready[1].network);
    // Both have already bound 1455 before either callback is sent.
    for (const owner of owners) owner.child.stdin!.write("callback\n");
    assert.deepEqual(await Promise.all(observed.map((seen) => seen.ended)), [0, 0]);
    for (const [index, id] of ["first", "second"].entries()) {
      assert.match(observed[index].output(), new RegExp(`${id}:/auth/callback`));
    }
    assert.equal(networkIdentity(), host);
  } finally { await Promise.all(owners.map((owner) => owner.stop())); }
});

it("preserves the real case diagnostics and propagates a private test's failure", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-network-test-"));
  const file = join(root, "nested.test.ts");
  writeFileSync(file, `import { it } from "node:test";
import { networkTests } from ${JSON.stringify(new URL("./network.ts", import.meta.url).href)};
networkTests(import.meta.url, () => {
  it("the original private case", () => { throw new Error("the original private failure"); });
});\n`);
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.DESKTOP_TEST_PARENT_NETWORK;
  const owner = Owned.spawn(process.execPath, ["--test", file], { env, stdio: ["ignore", "pipe", "pipe"] });
  const seen = observe(owner);
  try {
    assert.equal(await seen.ended, 1);
    assert.match(seen.output(), /the original private case/);
    assert.match(seen.output(), /the original private failure/);
    assert.match(seen.output(), /account journeys failed: 1/);
  } finally {
    await owner.stop();
    rmSync(root, { recursive: true, force: true });
  }
});
