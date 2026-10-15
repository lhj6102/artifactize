// One suite owns its build, the exact frontend input, and the executables every worker uses.
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { cleanup, command, prepare, readCandidate, retain } from "./build.ts";

const mode = process.argv[2] ?? "suite";
if (mode === "cleanup") {
  await cleanup(readCandidate(process.argv[3]).root);
} else {
  const candidate = await prepare();
  if (mode === "prepare") {
    // The verification builder owns this declared input until its Run becomes terminal.
    retain(candidate.root);
    console.log(candidate.manifest);
  } else if (mode === "suite") {
    try {
      const tests = readdirSync(import.meta.dirname).filter((name) => name.endsWith(".test.ts")).sort().map((name) => join(import.meta.dirname, name));
      await command(candidate.root, process.execPath, ["--test", "--test-concurrency=1", ...tests], {
        ...process.env, DESKTOP_TEST_CANDIDATE: candidate.manifest,
      });
    } finally { await cleanup(candidate.root); }
  } else {
    await cleanup(candidate.root);
    throw new Error(`unknown desktop test runner mode: ${mode}`);
  }
}
