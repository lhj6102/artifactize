// The Agent session's launch and declaration tools drive the real app and real fixture bytes.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { after, before, describe, it } from "node:test";
import { createGraph } from "./fixture.ts";
import { checkBuild, LAUNCH_TIMEOUT } from "./driver.ts";
import { Owned } from "./owned.ts";

describe("desktop Agent session", () => {
  const harness = resolve(import.meta.dirname, "../../harness/harness.mts");
  let owner: Owned;
  let env: NodeJS.ProcessEnv;
  function call(mode: string, args: Record<string, unknown> = {}): string {
    const output = execFileSync(process.execPath, [harness, mode], {
      env, input: JSON.stringify({ args }), encoding: "utf8", timeout: mode === "launch" ? LAUNCH_TIMEOUT : 30000,
    });
    if (mode === "stop") return output;
    const reply = JSON.parse(output) as { isError?: boolean; content: { text: string }[] };
    assert.equal(reply.isError ?? false, false, output);
    return reply.content.map((part) => part.text).join("\n");
  }

  before(async () => {
    checkBuild();
    const directory = join(createGraph().root, "session");
    mkdirSync(directory);
    env = { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, ARTIFACTIZE_REMOTE: "off" };
    owner = Owned.spawn(process.execPath, [harness, "start"], { env, stdio: ["ignore", "pipe", "inherit"] });
    await new Promise<void>((done, fail) => {
      const lines = createInterface({ input: owner.child.stdout! });
      const deadline = setTimeout(() => finish(new Error("desktop Agent session did not become ready")), LAUNCH_TIMEOUT);
      const exited = (code: number | null) => finish(new Error(`desktop Agent session exited ${code} before ready`));
      function finish(error?: Error) {
        clearTimeout(deadline);
        lines.close();
        owner.child.off("exit", exited);
        if (error) fail(error);
        else done();
      }
      lines.once("line", (line) => line === "ready" ? finish() : finish(new Error(`unexpected readiness: ${line}`)));
      owner.child.once("exit", exited);
    });
  });
  after(async () => {
    if (!owner) return;
    try { call("stop"); } finally { await owner.stop(); }
  });

  it("relaunches each real entry point and shows a changed declaration only after current-graph refresh", () => {
    assert.match(call("snapshot"), /signoff/);
    assert.match(call("launch", { screen: "graph" }), /Current Artifact graph/);
    assert.match(call("cli", { args: ["run", "list", "--json"] }), /--- stdout\n\[\]/, "graph state has no saved Run");
    const changed = call("change-declaration");
    assert.match(changed, /Docs criterion revision 1/);
    assert.doesNotMatch(call("snapshot"), /Docs criterion revision 1/, "no refresh was simulated");
    const refreshed = call("act", { action: "key", key: "Control+r" });
    const control = refreshed.match(/\[(\d+)\] summary "Structure and evaluation criteria/);
    assert.ok(control, refreshed);
    assert.match(call("act", { action: "click", element: Number(control[1]) }), /Docs criterion revision 1/);
    const review = call("launch", { screen: "review" });
    assert.match(review, /Search requests/);
    assert.match(review, /release\/signoff/);
    assert.match(call("launch", { screen: "monitor" }), /Run run-/);
    assert.match(call("snapshot"), /signoff/);
  });
});
