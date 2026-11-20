// A provider speaking the real Responses API, owned by the scenario until the app exits.
import { serveLoopbackHttp } from "../portable.ts";
import type { Owned } from "./owned.ts";
import { artifactize, type Fixture } from "./repo.ts";

export const CHAT_MODEL = "fixture-chat";
const DEFAULT_MODEL = "fixture-small";
export const WORK_PROMPT = "Read the release notes and verify the worktree";
export const STOP_PROMPT = "Wait for my next instruction";
export const QUIT_PROMPT = "Keep working until I quit";
export const CONTINUE_PROMPT = "Continue after the stopped turn";
export const CONTINUE_ANSWER = "The conversation can continue.";
export const CHAT_ANSWER = "The notes were read and Verify was started.";

function call(id: string, name: string, args: object) {
  return { type: "function_call", id: `fc_${id}`, call_id: id, name, arguments: JSON.stringify(args), status: "completed" };
}

export async function installChatProvider(owned: Owned, fixture: Fixture): Promise<void> {
  const endpoint = await serveLoopbackHttp((request, response) => {
    if (request.method === "GET" && request.url === "/v1/models") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ object: "list", data: [DEFAULT_MODEL, CHAT_MODEL].map((id) => ({ id, object: "model", created: 1, owned_by: "fixture" })) }));
      return;
    }
    if (request.method !== "POST" || request.url !== "/v1/responses") {
      response.writeHead(404).end();
      return;
    }
    let body = "";
    request.setEncoding("utf8").on("data", (chunk: string) => { body += chunk; });
    request.once("end", () => {
      const selection = (JSON.parse(body) as { model?: string }).model;
      if (selection !== CHAT_MODEL) {
        response.writeHead(400, { "Content-Type": "application/json" });
        response.end(JSON.stringify({ error: { message: `Expected the selected model ${CHAT_MODEL}, got ${selection ?? "no model"}` } }));
        return;
      }
      if (body.includes(QUIT_PROMPT)) return; // Quitting the app ends this response.
      const resumed = body.includes(CONTINUE_PROMPT);
      if (!resumed && body.includes(STOP_PROMPT)) return; // The user owns cancellation of this response.
      let number = 1;
      let output: object[];
      if (resumed || body.includes('"call_id":"verify-notes"')) {
        number = 3;
        output = [{ type: "message", id: "msg_notes", role: "assistant", status: "completed", content: [{ type: "output_text", text: resumed ? CONTINUE_ANSWER : CHAT_ANSWER, annotations: [] }] }];
      } else if (body.includes('"call_id":"read-notes"')) {
        number = 2;
        output = [call("verify-notes", "verify_start", { scope: { kind: "artifact", artifactId: "docs" } })];
      } else if (body.includes(WORK_PROMPT)) {
        output = [call("read-notes", "read", { path: "release/notes.md" })];
      } else {
        response.writeHead(422).end();
        return;
      }
      const events = number === 3 ? [] : ["response.output_item.added", "response.output_item.done"].map((type) => ({ type, item: output[0], output_index: 0 }));
      const complete = { type: "response.completed", response: { id: `resp_${number}`, object: "response", created_at: 1, model: CHAT_MODEL, status: "completed", output,
        usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } } };
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.end([...events, complete].map((event, sequence_number) => `event: ${event.type}\ndata: ${JSON.stringify({ ...event, sequence_number })}\n\n`).join(""));
    });
  });
  owned.defer(endpoint.close);
  const added = artifactize(fixture, ["provider", "add", "fixture", "--api", "openai-responses", "--url", `${endpoint.url}/v1`]);
  if (added.status !== 0) throw new Error(`could not register the fixture provider: ${added.stderr}`);
}
