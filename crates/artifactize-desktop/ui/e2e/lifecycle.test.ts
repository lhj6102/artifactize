// Observable fixture boundaries: owned window lifetime, complete failure reporting,
// isolated Git writes, and ownership even when native registration fails. No app build.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { it } from "node:test";
import { windowClosed } from "./driver.ts";
import { groupAlive, Owned, withCleanup } from "./owned.ts";

it("requires an owned app lifetime rather than treating a session response or error as closure", async () => {
  await assert.rejects(windowClosed({ getWindowHandles: async () => [] }), /requires an owned native app session/);
  await assert.rejects(windowClosed({ getWindowHandles: async () => { throw new Error("transport failed"); } }), /requires an owned native app session/);
});

it("retains the primary error and attempts every failed owned teardown", async () => {
  const primary = new Error("the test failed");
  const native = new Error("the native stop failed");
  const lease = new Error("the lease stop failed");
  let drained = false;
  await assert.rejects(withCleanup(() => { throw primary; }, () => { throw native; }, () => {
    drained = true;
    throw lease;
  }), (error) => error instanceof AggregateError && error.errors[0] === primary && error.errors[1] === native && error.errors[2] === lease);
  assert.equal(drained, true, "an earlier teardown failure does not bypass later owners");
});

it("keeps Git repository/index/hooks/config routing inside its fixture", () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-git-boundary-"));
  const foreign = join(root, "foreign");
  const hooks = join(root, "foreign-hooks");
  const marker = join(root, "foreign-hook-ran");
  const index = join(root, "foreign-index");
  mkdirSync(foreign);
  mkdirSync(hooks);
  writeFileSync(join(foreign, "sentinel"), "unchanged");
  writeFileSync(join(hooks, "pre-commit"), `#!/bin/sh\ntouch '${marker}'\nexit 1\n`, { mode: 0o700 });
  const config = join(root, "foreign-config");
  writeFileSync(config, `[core]\nhooksPath = ${JSON.stringify(hooks)}\n[commit]\ngpgSign = true\n`);
  const source = `import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import assert from "node:assert/strict";
import { git } from ${JSON.stringify(new URL("./environment.ts", import.meta.url).href)};
const repo = ${JSON.stringify(join(root, "repo"))};
mkdirSync(repo);
writeFileSync(repo + "/input", "owned");
git(repo, "init", "-q", "--initial-branch=master", repo);
git(repo, "-C", repo, "add", ".");
git(repo, "-C", repo, "commit", "-qm", "Owned fixture");
git(repo, "-C", repo, "worktree", "add", "-q", "-b", "other", ${JSON.stringify(join(root, "other"))});
assert.ok(existsSync(repo + "/.git/index"));
assert.ok(existsSync(${JSON.stringify(join(root, "other", ".git"))}));`;
  try {
    execFileSync(process.execPath, ["--input-type=module", "-e", source], {
      env: { ...process.env, GIT_DIR: foreign, GIT_WORK_TREE: foreign, GIT_INDEX_FILE: index,
        GIT_CONFIG_GLOBAL: config, GIT_CONFIG_SYSTEM: config, GIT_CONFIG_NOSYSTEM: "0",
        GIT_CONFIG_COUNT: "1", GIT_CONFIG_KEY_0: "core.hooksPath", GIT_CONFIG_VALUE_0: hooks },
      encoding: "utf8",
    });
    assert.deepEqual(readdirSync(foreign), ["sentinel"], "the inherited repository was not initialized or modified");
    assert.equal(existsSync(index), false, "staging did not use the inherited index");
    assert.equal(existsSync(marker), false, "commit did not invoke inherited hooks or signing");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

it("joins an acquired process even when registering its group fails", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-registration-"));
  mkdirSync(join(root, "groups")); // appendFile must fail after the process was acquired.
  const owner = Owned.spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    env: { ...process.env, DESKTOP_TEST_RESOURCES: root }, stdio: ["ignore", "ignore", "ignore"],
  });
  const acquisition = await new Promise<Error>((done) => owner.child.once("error", done));
  await withCleanup(async () => {
    assert.equal((acquisition as NodeJS.ErrnoException).code, "EISDIR");
    await assert.rejects(owner.stop(), (error) => error === acquisition);
    assert.equal(groupAlive(owner.group), false, "failed registration still joins the actual acquired group");
  }, async () => {
    try { await owner.stop(); } catch (error) { if (error !== acquisition) throw error; }
    if (groupAlive(owner.group)) throw new Error("preserve inputs of an acquired group that has not stopped");
    rmSync(root, { recursive: true, force: true });
  });
});
