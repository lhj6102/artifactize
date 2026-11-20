// What Point appended to a review's Notes stands out there until the Notes are next edited
// (D-REV-4, D-VIEW-2). The text is the library draft's; which lines of it were pointed is what
// the windows tell one another: a viewer when Point appended a line, any window when its Notes
// field was edited.
import { emit, listen } from "@tauri-apps/api/event";
import { subscribe } from "../ipc/client";

/** A viewer's Point appended `text` to review `review`'s Notes. */
const POINTED = "notes-pointed";
/** Review `review`'s Notes were edited in some window. */
const EDITED = "notes-edited";

export type Pointed = { review: string; text: string };

export function announcePointed(review: string, text: string): void {
  void emit(POINTED, { review, text } satisfies Pointed).catch(() => {});
}

export function announceEdited(review: string): void {
  void emit(EDITED, { review }).catch(() => {});
}

/** Follow what Point appends and what edits Notes, in any window, until the returned function
 * is called. */
export function followPointed(pointed: (pointed: Pointed) => void, edited: (review: string) => void): () => void {
  const unpointed = subscribe<Pointed>((handler) => listen(POINTED, handler), pointed);
  const unedited = subscribe<{ review: string }>((handler) => listen(EDITED, handler), (payload) => edited(payload.review));
  return () => {
    unpointed();
    unedited();
  };
}

/** The lines of `notes`, each with whether it is one of the `pointed` texts. */
export function pointedLines(notes: string, pointed: readonly string[]): { text: string; pointed: boolean }[] {
  const marked = new Set(pointed.map((text) => text.trim()).filter(Boolean));
  return notes.split("\n").map((text) => ({ text, pointed: marked.has(text.trim()) && text.trim() !== "" }));
}
