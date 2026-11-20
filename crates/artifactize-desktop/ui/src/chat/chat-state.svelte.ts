// Chat mode's own facts: the sessions the library lists, the session shown with its transcript,
// the Run reference to bring into view, and what is typed in the composer. One owner, kept in the
// AppState; sessions, turns and Run cards are the library's, read through the IPC.
import type { ChatSession, ChatView } from "../ipc/client";

export class ChatModeState {
  /** The library's sessions, newest activity first. */
  sessions = $state.raw<ChatSession[]>([]);
  /** The library has answered the list at least once. */
  listed = $state(false);
  /** The session shown, by its ID. */
  selected = $state<string | null>(null);
  /** The shown session as the library last read it. */
  view = $state.raw<ChatView | null>(null);
  /** A Run reference to bring into view once the shown session is read (J-RUN-ORIGIN). */
  reveal = $state<string | null>(null);
  /** What is typed in each session's composer, by session: kept while another is shown. */
  readonly typed: Record<string, string> = $state({});
  /** What went wrong, for the main area's notice. */
  error = $state<string | null>(null);
  noticeOpen = $state(false);
  /** The New chat dialog is open. */
  creating = $state(false);
  /** The model menu is open. */
  picking = $state(false);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; composer?: HTMLTextAreaElement } = $state({});
}
