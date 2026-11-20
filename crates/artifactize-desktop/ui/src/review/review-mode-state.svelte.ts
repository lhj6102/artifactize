// Review mode's own facts: where focus is, the review chosen in its list, and what its main area
// shows: a Human review (actionable or completed), a request opened by ID read-only, or nothing
// yet. One owner, kept in the AppState; the reviews themselves are the AppState's.
import type { DetailView } from "../ipc/client";

export type ReviewFocus = "pane" | "main";

/** What Review's main area shows. */
export type ReviewShown = { kind: "none" } | { kind: "review"; id: string } | { kind: "request"; detail: DetailView };

export class ReviewModeState {
  focus = $state<ReviewFocus>("pane");
  /** The review chosen in the list, by its request ID. */
  selected = $state<string | null>(null);
  shown = $state.raw<ReviewShown>({ kind: "none" });
  /** The read-only request shows its log rather than its Summary. */
  folds = $state({ log: false });
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** What went wrong opening a reference, for the main area's notice. */
  error = $state<string | null>(null);
  /** What the launch asked for was opened: only the first snapshot opens it. */
  started = $state(false);
  /** The snapshot revision a request shown by ID was last read for; never drawn. */
  followed = $state(0);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement } = $state({});
}
