// Backend answers for component tests: what the commands would return for one request.
import type { DraftChange, FieldValue, FormView, HumanView, ReviewDraft, Verdict } from "../ipc/client";

/** The library's draft of one review, kept as the backend keeps it: `answer` takes the draft
 * commands (`human_draft`, `human_draft_edit`) and returns what the backend
 * would, `undefined` for any other command. A list item added is `empty` of that list. */
export function draftLibrary(green: FormView = greenForm, red: FormView = redForm, empty: Record<string, unknown> = {}) {
  const draft: ReviewDraft = { verdict: null, points: [], green: structuredClone(green), red: structuredClone(red) };
  const form = (verdict: Verdict) => (verdict === "GREEN" ? draft.green : draft.red);
  const edit = (verdict: Verdict, name: string, change: (value: FieldValue) => FieldValue) => {
    const fields = form(verdict).draft.fields;
    const at = fields.findIndex(([field]) => field === name);
    fields[at] = [name, change(fields[at][1])];
  };
  const answer = (cmd: string, args: unknown): ReviewDraft | undefined => {
    if (cmd === "human_draft") return structuredClone(draft);
    if (cmd !== "human_draft_edit") return undefined;
    const change = (args as { change: DraftChange }).change;
    if (change.kind === "verdict") draft.verdict = change.verdict;
    else if (change.kind === "field") edit(change.verdict, change.name, () => change.value);
    else {
      const item = change.kind === "removeItem" ? change.item : null;
      edit(change.verdict, change.name, (value) => {
        const items = "value" in value && Array.isArray(value.value) ? (value.value as unknown[]) : [];
        const next = item === null ? [...items, empty[change.name] ?? ""] : items.toSpliced(item, 1);
        return { ...value, value: value.kind === "groups" && next.length === 0 ? [empty[change.name]] : next } as FieldValue;
      });
    }
    return structuredClone(draft);
  };
  return { draft, answer };
}

export function humanView(stage: HumanView["stage"], overrides: Partial<HumanView> = {}): HumanView {
  return {
    id: "run-a-2",
    target: { run: "run-a", node: "e:release/signoff" },
    notes: null,
    title: "release/signoff · A person approves the release",
    artifact: "release",
    meta: stage === "claim" ? "WAITING_HUMAN · unclaimed · waiting 3s" : "WAITING_HUMAN · claimed by you (alice) · waiting 3s",
    shared: null,
    instruction: "Read the release notes and approve them.",
    technical: [{ key: "Request", value: "run-a-2" }],
    mark: { status: "WAITING_HUMAN", glyph: "?" },
    stage,
    claim: stage === "review" ? { reviewer: "alice", mine: true } : null,
    reviewer: "alice",
    tools: [
      { name: "notes_release", artifact: "release", operation: "notes", kind: "output", description: "Print the notes.", action: "$ cat notes.md", error: null },
    ],
    recorded: true,
    result: null,
    completed: null,
    generation: 0,
    read: 0,
    ...overrides,
  };
}

export const redForm: FormView = {
  verdict: "RED",
  fields: [{ name: "reason", required: true, hint: "string, minLength 3", input: { kind: "text" } }],
  draft: { verdict: "RED", fields: [["reason", { kind: "text", value: "" }]] },
};

export const greenForm: FormView = {
  verdict: "GREEN",
  fields: [
    { name: "approved", required: true, hint: "", input: { kind: "fixed", value: "true" } },
    { name: "comment", required: true, hint: "string", input: { kind: "text" } },
  ],
  draft: {
    verdict: "GREEN",
    fields: [
      ["approved", { kind: "fixed" }],
      ["comment", { kind: "text", value: "" }],
    ],
  },
};
