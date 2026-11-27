// Backend answers for component tests: what the commands would return for one request.
import type { DraftChange, HumanView, Readiness, ReviewDraft, ReviewPoint, Verdict } from "../ipc/client";

/** Whether `verdict` can be submitted as the draft stands, as the library says: GREEN takes no
 * issue, RED needs one. */
function readiness(draft: ReviewDraft, verdict: Verdict): Readiness {
  if (verdict === "RED" && draft.issues.length === 0) return { kind: "blocked", reason: "RED needs at least one issue." };
  if (verdict === "GREEN" && draft.issues.length > 0) return { kind: "blocked", reason: "GREEN takes no issues: remove them, or submit RED with them." };
  return { kind: "ready" };
}

/** The library's draft of one review, kept as the backend keeps it: `answer` takes the draft
 * commands (`human_draft`, `human_draft_edit`, `human_draft_remove_point`) and returns what the
 * backend would, `undefined` for any other command. */
export function draftLibrary() {
  const draft: ReviewDraft = {
    verdict: null, issues: [], readiness: { green: { kind: "ready" }, red: { kind: "ready" } }, points: [],
  };
  const answered = () => {
    draft.readiness = { green: readiness(draft, "GREEN"), red: readiness(draft, "RED") };
    return structuredClone(draft);
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
    if (change.kind === "verdict") draft.verdict = change.verdict;
    else if (change.kind === "addIssue") {
      if (change.text.trim() === "") throw { kind: "invalid", message: "An issue needs text." };
      draft.issues.push(change.text);
    } else {
      if (draft.issues[change.index] !== change.previous) throw { kind: "conflict", message: "That issue changed meanwhile." };
      if (change.kind === "editIssue") draft.issues[change.index] = change.text;
      else draft.issues.splice(change.index, 1);
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
