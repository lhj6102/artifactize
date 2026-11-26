// Stand-in `agy`: the CLI's version flag, manual sign-in on stdio, and ACP on stdio.
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { setTimeout as delay } from "node:timers/promises";
import { AGY_MODELS, AGY_PARTIAL, AGY_PROMPT, SIGN_IN_CODE, SIGN_IN_URL } from "./antigravity.ts";

const signedIn = process.env.DESKTOP_TEST_AGY_SIGNED_IN;
const version = process.env.DESKTOP_TEST_AGY_VERSION;
const answer = process.env.DESKTOP_TEST_AGY_ANSWER;
if (!signedIn || !version || !answer) throw new Error("agy must run in its fixture environment");
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
} else if (args === "acp" || args === "acp --sandbox --include-default-customizations=false") {
  let model = AGY_MODELS[0].value;
  const config = (currentValue: string) => [{ id: "model", name: "Model", category: "model", type: "select", currentValue, options: AGY_MODELS }];
  for await (const line of lines()) {
    const { id, method, params } = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
    let result: object;
    if (method === "initialize") result = { protocolVersion: 1, agentCapabilities: { auth: { logout: {} } } };
    else if (method === "logout") {
      rmSync(signedIn, { force: true });
      result = {};
    } else if (method === "session/new" && existsSync(signedIn)) result = { sessionId: "agy-fixture", configOptions: config(model) };
    else if (method === "session/set_config_option" && params.sessionId === "agy-fixture" && params.configId === "model" && typeof params.value === "string" && AGY_MODELS.some(({ value }) => value === params.value)) {
      model = params.value;
      result = { configOptions: config(model) };
    } else if (method === "session/prompt" && existsSync(signedIn) && params.sessionId === "agy-fixture"
      && model === AGY_MODELS[1].value && Array.isArray(params.prompt) && params.prompt.some(({ type, text }) => type === "text" && typeof text === "string" && text.includes(AGY_PROMPT))) {
      const chunk = (text: string) => console.log(JSON.stringify({ jsonrpc: "2.0", method: "session/update", params: {
        sessionId: "agy-fixture", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } },
      } }));
      chunk(AGY_PARTIAL);
      const deadline = Date.now() + 30000;
      while (!existsSync(answer) && Date.now() < deadline) await delay(25);
      if (!existsSync(answer)) throw new Error("the case did not release the ACP response");
      chunk(" over ACP.");
      result = { stopReason: "end_turn" };
    } else {
      const error = method === "session/new" ? { code: -32000, message: "Authentication required" } : { code: -32601, message: "Unsupported fixture request" };
      console.log(JSON.stringify({ jsonrpc: "2.0", id, error }));
      continue;
    }
    console.log(JSON.stringify({ jsonrpc: "2.0", id, result }));
  }
} else process.exitCode = 2;
