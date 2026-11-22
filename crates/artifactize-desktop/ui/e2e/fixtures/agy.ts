// Stand-in `agy`: the CLI's version flag, manual sign-in on stdio, and ACP on stdio.
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { AGY_MODELS, SIGN_IN_CODE, SIGN_IN_URL } from "./antigravity.ts";

const signedIn = process.env.DESKTOP_TEST_AGY_SIGNED_IN;
const version = process.env.DESKTOP_TEST_AGY_VERSION;
if (!signedIn || !version) throw new Error("agy must run in its fixture environment");
const args = process.argv.slice(2).join(" ");
const lines = () => createInterface({ input: process.stdin, crlfDelay: Infinity });

if (args === "--version") {
  console.log(`agy ${readFileSync(version, "utf8")}`);
} else if (args === "acp --login") {
  console.log(`Open this URL to sign in: ${SIGN_IN_URL}`);
  // EOF or process cancellation does not sign the fixture in.
  for await (const code of lines()) {
    if (code === SIGN_IN_CODE) writeFileSync(signedIn, "signed in");
    else process.exitCode = 1;
    break;
  }
} else if (args === "acp --sandbox --include-default-customizations=false") {
  const config = (currentValue: string) => [{ id: "model", category: "model", type: "select", currentValue, options: AGY_MODELS }];
  for await (const line of lines()) {
    const { id, method, params } = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
    let result: object;
    if (method === "initialize") result = { protocolVersion: 1, agentCapabilities: { auth: { logout: {} } } };
    else if (method === "logout") {
      rmSync(signedIn, { force: true });
      result = {};
    } else if (method === "session/new" && existsSync(signedIn)) result = { sessionId: "agy-fixture", configOptions: config(AGY_MODELS[0].value) };
    else if (method === "session/set_config_option" && params.sessionId === "agy-fixture" && params.configId === "model" && typeof params.value === "string" && AGY_MODELS.some(({ value }) => value === params.value)) {
      result = { configOptions: config(params.value) };
    } else {
      const error = method === "session/new" ? { code: -32000, message: "Authentication required" } : { code: -32601, message: "Unsupported fixture request" };
      console.log(JSON.stringify({ jsonrpc: "2.0", id, error }));
      continue;
    }
    console.log(JSON.stringify({ jsonrpc: "2.0", id, result }));
  }
} else process.exitCode = 2;
