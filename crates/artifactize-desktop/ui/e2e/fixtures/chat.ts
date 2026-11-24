// A provider speaking the real Responses API, owned by the scenario until the app exits.
import { exists, nativePath, serveLoopbackHttp, writeText } from "../portable.ts";
import type { Owned } from "./owned.ts";
import { artifactize, type Fixture } from "./repo.ts";

export const CHAT_MODEL = "fixture-chat";
export const DEFAULT_MODEL = "fixture-small";
export const WORK_PROMPT = "Read the release notes and verify the worktree";
export const PARTIAL_ANSWER = "I am still working on the notes.";
export const NEXT_DELTA = "The next part of this turn has arrived.";
export const CHAT_GATE = "next-delta";
export const STOP_STREAM_CLOSED = "stop-stream-closed";
export const STOP_PROMPT = "Wait for my next instruction";
export const QUIT_PROMPT = "Keep working until I quit";
export const CONTINUE_PROMPT = "Continue after the stopped turn";
export const CONTINUE_ANSWER = "The conversation can continue.";
export const CHAT_ANSWER = "The notes were read and Verify was started.";
export const GRAPH_PROMPT = "Summarize what you see on the graph";
export const GRAPH_GATE = "graph-answer";
export const GRAPH_ANSWER = "I see the Current while we work.";
export const MOVE_PROMPT = "Read the marker in the moved checkout";
export const MOVE_FILE = "elsewhere-only/marker.txt";
export const MOVE_MARKER = "Moved checkout marker.";
export const MOVE_ANSWER = "The moved checkout marker was read.";

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
      const askedMove = body.includes(MOVE_PROMPT);
      const resumed = !askedMove && body.includes(CONTINUE_PROMPT);
      const askedGraph = body.includes(GRAPH_PROMPT);
      if (askedGraph) {
        const gate = nativePath(fixture.root, GRAPH_GATE);
        const release = setInterval(() => {
          if (!exists(gate)) return;
          clearInterval(release);
          const complete = { type: "response.completed", sequence_number: 0, response: { id: "resp_graph", object: "response", created_at: 1, model: CHAT_MODEL, status: "completed",
            output: [{ type: "message", id: "graph_answer", role: "assistant", status: "completed", content: [{ type: "output_text", text: GRAPH_ANSWER, annotations: [] }] }],
            usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } } };
          response.writeHead(200, { "Content-Type": "text/event-stream" });
          response.end(`event: ${complete.type}\ndata: ${JSON.stringify(complete)}\n\n`);
        }, 25);
        response.once("close", () => clearInterval(release));
        return;
      }
      if (!resumed && body.includes(STOP_PROMPT)) {
        const early = [
          { type: "response.output_item.added", output_index: 0, item: { type: "message", id: "msg_partial", role: "assistant", status: "in_progress", content: [] } },
          { type: "response.output_text.delta", item_id: "msg_partial", output_index: 0, content_index: 0, delta: PARTIAL_ANSWER },
        ];
        response.writeHead(200, { "Content-Type": "text/event-stream" });
        response.write(early.map((event, sequence_number) => `event: ${event.type}\ndata: ${JSON.stringify({ ...event, sequence_number })}\n\n`).join(""));
        const gate = nativePath(fixture.root, CHAT_GATE);
        const release = setInterval(() => {
          if (!exists(gate)) return;
          clearInterval(release);
          const delta = { type: "response.output_text.delta", item_id: "msg_partial", output_index: 0, content_index: 0,
            delta: `\n\n${NEXT_DELTA}\n\n${"Streaming work.\n\n".repeat(60)}`, sequence_number: early.length };
          response.write(`event: ${delta.type}\ndata: ${JSON.stringify(delta)}\n\n`);
        }, 25);
        response.once("close", () => {
          clearInterval(release);
          writeText(nativePath(fixture.root, STOP_STREAM_CLOSED), "closed\n");
        });
        return; // Only Stop or fixture teardown ends this stream; no completion is sent.
      }
      let number = 1;
      let output: object[];
      if (askedMove) {
        const readMoved = body.includes('"call_id":"read-moved"');
        if (readMoved && !body.includes(MOVE_MARKER)) {
          response.writeHead(422).end();
          return;
        }
        number = readMoved ? 3 : 1;
        output = readMoved
          ? [{ type: "message", id: "msg_moved", role: "assistant", status: "completed", content: [{ type: "output_text", text: MOVE_ANSWER, annotations: [] }] }]
          : [call("read-moved", "read", { path: MOVE_FILE })];
      } else if (resumed || body.includes('"call_id":"verify-notes"')) {
        number = 3;
        output = [{ type: "message", id: "msg_notes", role: "assistant", status: "completed", content: [{ type: "output_text", text: resumed ? CONTINUE_ANSWER : `${CHAT_ANSWER}\n\n${"More release notes.\n\n".repeat(60)}`, annotations: [] }] }];
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
