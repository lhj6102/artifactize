// A runner-owned process tree for the cancellation regression: `owner` starts one
// `descendant`, which reports its process id to the test and stays alive until stopped.
import { nodeProgram, processId, runProgramAsync, sendLoopback, serveLoopback, suitePath } from "../platform.ts";

const [role, receiver] = process.argv.slice(2);
if (role === "owner") {
  // The owner stays alive while its descendant runs; the test must stop both.
  await runProgramAsync(nodeProgram, [suitePath("domain-fixtures", "tree.ts"), "descendant", receiver]);
} else if (role === "descendant") {
  // The open listener keeps the descendant running; only the runner's cancellation ends it.
  await serveLoopback();
  await sendLoopback(Number(receiver), JSON.stringify(processId()));
} else {
  throw new Error(`unknown process tree role ${role}`);
}
