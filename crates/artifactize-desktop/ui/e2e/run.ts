// One suite owns its build, the exact frontend input, and the executables every worker uses.
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { cleanup, prepare, readCandidate, retain, runSuite } from "./build.ts";

const mode = process.argv[2] ?? "suite";
if (mode === "cleanup") {
  await cleanup(readCandidate(process.argv[3]).root);
} else {
  const candidate = await prepare();
  if (mode === "prepare") {
    // The verification builder owns this declared input until its Run becomes terminal.
    await retain(candidate.root);
    console.log(candidate.manifest);
  } else if (mode === "suite") {
    try {
      const tests = readdirSync(import.meta.dirname).filter((name) => name.endsWith(".test.ts")).sort().map((name) => join(import.meta.dirname, name));
      await runSuite(candidate.root, tests, {
        ...process.env, DESKTOP_TEST_CANDIDATE: candidate.manifest,
      });
    } catch (error) {
      try { await cleanup(candidate.root); }
      catch (shutdown) { throw new AggregateError([error, shutdown], "desktop suite and cleanup failed"); }
      throw error;
    }
    await cleanup(candidate.root);
  } else {
    await cleanup(candidate.root);
    throw new Error(`unknown desktop test runner mode: ${mode}`);
  }
}
