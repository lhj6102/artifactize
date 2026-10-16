import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const exec = promisify(execFile);
const helper = fileURLToPath(new URL("./publish.mjs", import.meta.url));
const crates = ["artifactize-tools", "artifactize-core", "artifactize-tui", "artifactize"];

test("each dry run verifies the current sources at the same package version", async () => {
  const root = await mkdtemp(join(tmpdir(), "artifactize-publish-test-"));
  try {
    await writeFile(join(root, "Cargo.toml"), '[workspace]\nmembers = ["crates/*"]\nresolver = "3"\n');
    for (const [index, name] of crates.entries()) {
      const dir = join(root, "crates", name);
      await mkdir(join(dir, "src"), { recursive: true });
      const dependency = index === 0 ? "" : `\n[dependencies]\n${crates[index - 1]} = { path = "../${crates[index - 1]}", version = "0.0.0-dev" }\n`;
      await writeFile(join(dir, "Cargo.toml"), `[package]\nname = "${name}"\nversion = "0.0.0-dev"\nedition = "2024"\nlicense = "Apache-2.0"\ndescription = "Disposable package verification fixture"\n${dependency}`);
    }
    const cwd = join(root, "crates", "artifactize");
    await copyFile(helper, join(cwd, "publish.mjs"));
    const options = {
      cwd,
      env: { ...process.env, CARGO_TARGET_DIR: join(root, "target"), CARGO_BUILD_BUILD_DIR: join(root, "shared-build") },
    };
    for (const api of ["first_candidate", "second_candidate"]) {
      for (const [index, name] of crates.entries()) {
        const body = index === 0 ? "42" : `${crates[index - 1].replaceAll("-", "_")}::${api}()`;
        await writeFile(join(root, "crates", name, "src", "lib.rs"), `pub fn ${api}() -> u32 { ${body} }\n`);
      }
      // The lock is an input, just as it is for the real workspace's --locked check.
      await exec("cargo", ["generate-lockfile", "--offline"], options);
      const { stderr } = await exec(process.execPath, ["publish.mjs"], options);
      for (const name of crates) assert.match(stderr, new RegExp(`Verifying ${name} v0\\.0\\.0-dev`));
      assert.deepEqual((await readdir(join(root, "target"))).filter((name) => name.startsWith("publish-")), []);
    }
    await writeFile(join(cwd, "src", "lib.rs"), "compile_error!(\"publication fixture fails\");\n");
    await assert.rejects(exec(process.execPath, ["publish.mjs"], options), (error) => {
      assert.equal(error.code, 101);
      assert.match(error.stderr, /publication fixture fails/);
      return true;
    });
    assert.deepEqual((await readdir(join(root, "target"))).filter((name) => name.startsWith("publish-")), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
