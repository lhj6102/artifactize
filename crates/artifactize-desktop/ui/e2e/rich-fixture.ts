// Actual saved Agent and runtime evidence, supplied by declared external fixture programs
// and a loopback provider. Neither the application IPC nor its state is mocked.
import { prepareFixture } from "./build-fixtures.ts";
import { cli, type Fixture } from "./fixture.ts";
import { environment, git } from "./environment.ts";
import { runtimeProfile } from "./native-fixtures.ts";
import { makeDirectory, nativePath, runProgramAsync, serveLoopbackHttp, writeText } from "./platform.ts";

export const LONG_NAME = `published-${"W".repeat(54)}`;
export const LONG_WORD = `code_${"unbroken".repeat(240)}`;
export const OWNER = "Orchard reviewer";
export const FINDING = "The displayed receipt includes its price.";
export const MODEL = "fixture-review-model";

export async function createRich(): Promise<{ fixture: Fixture; runs: string[]; release: () => Promise<void> }> {
  const answer = JSON.stringify({ verdict: "GREEN", owner: OWNER, summary: `Reviewed content.\n${LONG_WORD}\nReceipt review complete.`, findings: [
    { item: "R-RECEIPT", where: "receipt.md:12", change: FINDING },
    { item: "R-TOTAL", where: "receipt.md:18", change: "The total matches the line items." },
  ] });
  const provider = await serveLoopbackHttp((request, response) => {
    request.resume();
    if (request.method !== "POST" || !request.url?.endsWith("/responses")) {
      response.writeHead(404); response.end(); return;
    }
    const events = [{ type: "response.completed", sequence_number: 0, response: {
      id: "resp_fixture", object: "response", created_at: 1, model: MODEL, status: "completed",
      output: [{ type: "message", id: "msg_fixture", role: "assistant", status: "completed", content: [{ type: "output_text", text: answer, annotations: [] }] }],
      usage: { input_tokens: 48000, output_tokens: 200, total_tokens: 48200, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } },
    } }];
    response.writeHead(200, { "content-type": "text/event-stream" });
    response.end(events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(""));
  });
  const runs: string[] = [];
  try {
    const fixture = await prepareFixture(async (root) => {
      const repo = makeDirectory(nativePath(root, "repo"));
      const fixture = { root, repo, state: nativePath(root, "state") };
      const directory = makeDirectory(nativePath(repo, "report"));
      const other = Array.from({ length: 14 }, (_, index) => `
[evals.additional-${index}]
title = "Additional criterion ${index}"
profile = ${runtimeProfile()}
payload.instruction = "Check ${index}."
`).join("");
      writeText(nativePath(directory, "index.artf"), `name = ${JSON.stringify(LONG_NAME)}
[evals.agent]
title = "Review the published receipt"
profile = { kind = "agent", backend = "openai", model = "${MODEL}", timeout_ms = 60000 }
payload.instruction = ${JSON.stringify(`Review the receipt.\n${"Instruction paragraph.\n".repeat(50)}${LONG_WORD}`)}
pass_schema = { type = "object", properties = { summary = { type = "string" }, owner = { type = "string" }, findings = { type = "array", items = { type = "object", properties = { item = { type = "string" }, where = { type = "string" }, change = { type = "string" } }, required = ["item", "where", "change"], additionalProperties = false } } }, required = ["summary", "owner", "findings"], additionalProperties = false }
fail_schema = { type = "object", properties = { summary = { type = "string" } }, required = ["summary"], additionalProperties = false }
[evals.runtime]
title = "Print the receipt report"
profile = ${runtimeProfile("report")}
payload.instruction = "Record the actual report output."
${other}`);
      writeText(nativePath(makeDirectory(nativePath(repo, "basis")), "index.artf"), 'name = "basis"\nbasis = true\n');
      git(repo, "init", "-q", "--initial-branch=master", repo);
      const env = { ...environment(fixture), OPENAI_API_KEY: "fixture-not-a-secret", ARTIFACTIZE_OPENAI_BASE_URL: provider.url };
      // The second actual Run demonstrates reuse, preserving the original Agent transcript.
      for (let run = 0; run < 2; run += 1) {
        const result = await runProgramAsync(cli(), ["--repo", repo, "--state-dir", fixture.state, "verify", "--all", "--wait", "--json"], { env });
        if (result.status !== 0) throw new Error(`rich fixture verification failed: ${result.stdout}\n${result.stderr}`);
        runs.push((JSON.parse(result.stdout) as { id: string }).id);
      }
      return fixture;
    });
    return { fixture, runs, release: () => provider.close() };
  } catch (error) {
    await provider.close();
    throw error;
  }
}
