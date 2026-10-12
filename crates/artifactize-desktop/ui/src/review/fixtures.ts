// Backend answers for component tests: what the commands would return for one request.
import type { FormView, HumanView } from "../ipc/client";

export function humanView(stage: HumanView["stage"], overrides: Partial<HumanView> = {}): HumanView {
  return {
    id: "run-a-2",
    title: "release/signoff · A person approves the release",
    meta: stage === "claim" ? "WAITING_HUMAN · unclaimed · waiting 3s" : "WAITING_HUMAN · claimed by you (alice) · waiting 3s",
    shared: null,
    instruction: "Read the release notes and approve them.",
    technical: [{ key: "Request", value: "run-a-2" }],
    mark: { status: "WAITING_HUMAN", glyph: "?" },
    stage,
    claim: stage === "review" ? { reviewer: "alice", mine: true } : null,
    reviewer: "alice",
    tools: [
      { name: "notes_release", kind: "output", description: "Print the notes.", action: "$ cat notes.md", error: null },
    ],
    recorded: true,
    result: null,
    ...overrides,
  };
}

export const redForm: FormView = {
  verdict: "RED",
  fields: [{ name: "reason", required: true, hint: "string, minLength 3", input: { kind: "text" } }],
  json: null,
};

export const greenForm: FormView = {
  verdict: "GREEN",
  fields: [
    { name: "approved", required: true, hint: "", input: { kind: "fixed", value: "true" } },
    { name: "comment", required: true, hint: "string", input: { kind: "text" } },
  ],
  json: null,
};
