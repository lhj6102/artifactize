// A runner-owned process tree for the cancellation regression: `owner` starts one
// `descendant`, which reports its process id to the test and stays alive until stopped.
import { nodeProgram, processId, startOrphanFixture, suitePath } from "../platform.ts";
import { sendLoopback, serveLoopback } from "../portable.ts";

const [role, receiver] = process.argv.slice(2);
if (role === "owner") {
  // The owner stays alive while its descendant runs; the test must stop both.
  await startOrphanFixture(nodeProgram, [suitePath("domain-fixtures", "tree.ts"), "descendant", receiver]);
  await serveLoopback();
} else if (role === "owner-exits") {
  // Await the child's own announcement before returning. It closes its pipes and
  // stays alive, so a root-exit-only implementation cannot pass the regression.
  await new Promise<void>((done, fail) => {
    void serveLoopback((socket) => {
      let pid = "";
      socket.setEncoding("utf8");
      socket.on("data", (chunk: string) => { pid += chunk; });
      socket.once("end", () => { void sendLoopback(Number(receiver), pid).then(done, fail); });
      socket.once("error", fail);
    }).then(async (server) => {
      await startOrphanFixture(nodeProgram, [suitePath("domain-fixtures", "tree.ts"), "descendant", String(server.port)]);
    }).catch(fail);
  });
  process.exit(0);
} else if (role === "descendant") {
  // The open listener keeps the descendant running; only the runner's cancellation ends it.
  await serveLoopback();
  await sendLoopback(Number(receiver), JSON.stringify(processId()));
} else {
  throw new Error(`unknown process tree role ${role}`);
}
