// The stand-in program runtime evals and Human tools run (declarations.ts names it). A gate is a
// file its scenario writes; teardown ends every stand-in still waiting for one.
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const [operation, argument, record] = process.argv.slice(2);

/** Wait until the scenario writes `gate`, or its fixture is closed or gone. */
function opened(gate: string): Promise<void> {
  const root = dirname(gate);
  return new Promise((done) => {
    const poll = setInterval(() => {
      if (!existsSync(gate) && existsSync(root) && !existsSync(resolve(root, "closed"))) return;
      clearInterval(poll);
      done();
    }, 20);
  });
}

switch (operation) {
  case "pass":
    break;
  case "fail":
    process.exitCode = 1;
    break;
  case "say":
    process.stdout.write(`${argument}\n`);
    break;
  case "read":
    process.stdout.write(readFileSync(resolve(argument), "utf8"));
    break;
  case "report":
    // Long lines on both streams.
    for (let line = 0; line < 80; line += 1) {
      const long = line === 0 ? `code_${"unbroken".repeat(240)}` : "";
      process.stdout.write(`fixture stdout ${String(line).padStart(3, "0")} ${long}\n`);
      process.stderr.write(`fixture stderr ${String(line).padStart(3, "0")} ${long}\n`);
    }
    break;
  case "launch":
    // An application the reviewer opens: it leaves a mark that it ran.
    writeFileSync(resolve(argument), "launched");
    break;
  case "talk":
    // An application that tells its reviewer what happened: one line at once, then, once its
    // gate opens, one on each stream.
    process.stdout.write("opened the stand-in\n");
    await opened(resolve(argument));
    process.stdout.write("chose: B\n");
    process.stderr.write("warning: B is slower\n");
    break;
  case "slow":
    // Runs until it is cancelled (or its fixture closes); each run adds its process ID to
    // `<gate>.pids`, so its scenario can see the process end. A fixture already closed and
    // removed has no scenario to tell: the run ends as it would at its gate.
    try {
      appendFileSync(`${resolve(argument)}.pids`, `${process.pid}\n`);
    } catch {
      break;
    }
    await opened(resolve(argument));
    break;
  case "hold":
    // Runs past its eval's declared deadline: the product ends it as an ERROR.
    setInterval(() => {}, 1000);
    break;
  case "fingerprint": {
    // A fingerprint script that records each run, then answers once its gate opens; it fails
    // when the gate says so.
    appendFileSync(resolve(record), "run\n");
    const gate = resolve(argument);
    await opened(gate);
    if (existsSync(gate) && readFileSync(gate, "utf8") === "fail") process.exitCode = 1;
    else process.stdout.write("fixture:v1\n");
    break;
  }
  default:
    throw new Error(`unknown fixture operation ${operation}`);
}
