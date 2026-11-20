// Review mode's own facts: where focus is, its filter and the page of reviews the library found
// for it, the review chosen in its list, and what its main area shows: a Human review
// (actionable or completed), a request opened by ID read-only, or nothing yet. One owner, kept
// in the AppState; the reviews themselves are the AppState's.
import { REVIEW_DEFAULT_QUERY, type DetailView, type ReviewMatch } from "../ipc/client";

export type ReviewFocus = "pane" | "main";

/** What Review's main area shows. */
export type ReviewShown = { kind: "none" } | { kind: "review"; id: string } | { kind: "request"; detail: DetailView };

export class ReviewModeState {
  focus = $state<ReviewFocus>("pane");
  /** The filter the list was last searched with; the field shows it, overridden while typed. */
  query = $state<string>(REVIEW_DEFAULT_QUERY);
  /** The reviews the library found for the filter, newest first. */
  matches = $state.raw<ReviewMatch[]>([]);
  /** The library's next page of the filter, if there is one. */
  next = $state<number | null>(null);
  /** The library has answered the filter at least once. */
  searched = $state(false);
  /** The review chosen in the list, by its request ID. */
  selected = $state<string | null>(null);
  shown = $state.raw<ReviewShown>({ kind: "none" });
  /** Why the shown review, opened by a reference, is outside the filter; `null` while listed. */
  outside = $state<string | null>(null);
  /** The read-only request shows its log rather than its Summary. */
  folds = $state({ log: false });
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** What went wrong opening a reference or searching, for the main area's notice. */
  error = $state<string | null>(null);
  /** What the launch asked for was opened: only the first snapshot opens it. */
  started = $state(false);
  /** The snapshot revision a request shown by ID was last read for; never drawn. */
  followed = $state(0);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; filter?: HTMLInputElement } = $state({});
}
