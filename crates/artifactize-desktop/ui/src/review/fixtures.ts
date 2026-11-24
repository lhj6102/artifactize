// Backend answers for component tests: what the commands would return for one request.
import type { DraftChange, FieldValue, FormView, HumanView, Readiness, ReviewDraft, ReviewPoint, Verdict } from "../ipc/client";

/** Whether `verdict` can be submitted as the draft stands, as the library says: RED needs a
 * reason, and a required field of the verdict's own must not be empty. */
function readiness(draft: ReviewDraft, verdict: Verdict): Readiness {
  if (verdict === "RED" && draft.reasons.length === 0) return { kind: "blocked", reason: "RED needs a reason" };
  const form = verdict === "GREEN" ? draft.green : draft.red;
  for (const field of form.fields) {
    const value = form.draft.fields.find(([name]) => name === field.name)?.[1];
    const empty = value === undefined || ("value" in value && (value.value === "" || value.value === null));
    if (field.required && field.input.kind !== "fixed" && empty) return { kind: "blocked", reason: `${field.name} is required` };
  }
  return { kind: "ready" };
}

/** The library's draft of one review, kept as the backend keeps it: `answer` takes the draft
 * commands (`human_draft`, `human_draft_edit`, `human_draft_remove_point`) and returns what the backend
 * would, `undefined` for any other command. A list item added is `empty` of that list. */
export function draftLibrary(green: FormView = greenForm, red: FormView = redForm, empty: Record<string, unknown> = {}) {
  const draft: ReviewDraft = {
    verdict: null, reasons: [], readiness: { green: { kind: "ready" }, red: { kind: "ready" } },
    points: [], green: structuredClone(green), red: structuredClone(red),
  };
  const answered = () => {
    draft.readiness = { green: readiness(draft, "GREEN"), red: readiness(draft, "RED") };
    return structuredClone(draft);
  };
  const form = (verdict: Verdict) => (verdict === "GREEN" ? draft.green : draft.red);
  const edit = (verdict: Verdict, name: string, change: (value: FieldValue) => FieldValue) => {
    const fields = form(verdict).draft.fields;
    const at = fields.findIndex(([field]) => field === name);
    fields[at] = [name, change(fields[at][1])];
  };
  const answer = (cmd: string, args: unknown): ReviewDraft | undefined => {
    if (cmd === "human_draft") return answered();
    if (cmd === "human_draft_remove_point") {
      const { point } = args as { point: ReviewPoint };
      const at = draft.points.findIndex((saved) => saved.text === point.text && saved.source === point.source && saved.at === point.at);
      if (at >= 0) draft.points.splice(at, 1);
      return answered();
    }
    if (cmd !== "human_draft_edit") return undefined;
    const change = (args as { change: DraftChange }).change;
    if (change.kind === "addReason" || change.kind === "editReason" || change.kind === "removeReason") {
      if (change.kind === "addReason") {
        if (change.text.trim() === "") throw { kind: "invalid", message: "A reason needs text." };
        draft.reasons.push(change.text);
      } else {
        if (draft.reasons[change.index] !== change.previous) throw { kind: "conflict", message: "That reason changed meanwhile." };
        if (change.kind === "editReason") draft.reasons[change.index] = change.text;
        else draft.reasons.splice(change.index, 1);
      }
      return answered();
    }
    if (change.kind === "verdict") draft.verdict = change.verdict;
    else if (change.kind === "field") edit(change.verdict, change.name, () => change.value);
    else if (change.kind === "groupField") edit(change.verdict, change.name, (value) => {
      if (value.kind !== "groups") return value;
      return { kind: "groups", value: value.value.map((group, index) => index === change.item
        ? group.map(([name, child]) => [name, name === change.field ? change.value : child]) : group) };
    });
    else {
      const item = change.kind === "removeItem" ? change.item : null;
      edit(change.verdict, change.name, (value) => {
        const items = "value" in value && Array.isArray(value.value) ? (value.value as unknown[]) : [];
        const next = item === null ? [...items, empty[change.name] ?? ""] : items.toSpliced(item, 1);
        return { ...value, value: value.kind === "groups" && next.length === 0 ? [empty[change.name]] : next } as FieldValue;
      });
    }
    return answered();
  };
  return { draft, answer };
}

export function humanView(stage: HumanView["stage"], overrides: Partial<HumanView> = {}): HumanView {
  return {
    id: "run-a-2",
    owner: overrides.id ?? "run-a-2",
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

/** RED asks nothing but its reasons: its `reason` field is the reasons' own, left out. */
export const redForm: FormView = {
  verdict: "RED",
  fields: [],
  draft: { verdict: "RED", fields: [] },
};

/** GREEN asks nothing but its reasons: its constant and its `comment`, the reasons' own, left out. */
export const greenForm: FormView = {
  verdict: "GREEN",
  fields: [{ name: "approved", required: true, hint: "", input: { kind: "fixed", value: "true" } }],
  draft: { verdict: "GREEN", fields: [["approved", { kind: "fixed" }]] },
};

/** RED asking more than its reasons: a required score and an optional note. */
export const scoredRed: FormView = {
  verdict: "RED",
  fields: [
    { name: "score", required: true, hint: "integer", input: { kind: "integer" } },
    { name: "note", required: false, hint: "string", input: { kind: "text" } },
  ],
  draft: { verdict: "RED", fields: [["score", { kind: "integer", value: "" }], ["note", { kind: "text", value: "" }]] },
};
