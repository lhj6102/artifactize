// One Artifact Viewer window's facts: its viewer session, what its viewer is (a built-in one or
// a plugin's page), its context (the Artifact and the active review), whether it is pointing, and
// what went wrong. Everything it reads of the Artifact comes from the library through the viewer
// commands; Point appends through the library to the active review's draft.
import { createContext } from "svelte";
import { emit } from "@tauri-apps/api/event";
import { call, commands, message, type Commands, type ViewerContext, type ViewerEntry } from "../ipc/client";
import { announcePointed } from "../review/pointed";

/** What a viewer window tells the others about itself: the pet overlay points through it. */
export type Presence = { session: number; artifact: string; review: string | null; open: boolean; pointable: boolean };

/** The app-wide events between the windows. */
export const PRESENCE = "viewer-presence";
export const ASK = "viewer-ask";
export const ARM = "point-arm";

export class ViewerState {
  entry = $state.raw<ViewerEntry | null>(null);
  context = $state.raw<ViewerContext | null>(null);
  /** Pointing: the next click on a place in the Artifact appends it to the draft. */
  pointing = $state(false);
  error = $state<string | null>(null);
  /** A plugin's page: loading until it first reaches the bridge (it asks for `context` when it
   * starts), or silent when it finished loading without having asked for anything yet. */
  page = $state<"loading" | "connected" | "silent">("loading");
  /** The active review's eval, `{Artifact}/{eval}`, once read. */
  evalId = $state<string | null>(null);

  constructor(readonly session: number, readonly ipc: Commands = commands) {}

  /** Point is offered in the strip while this reviewer holds the active review: a built-in
   * viewer points by itself, a plugin's page once the strip tells it to (the bridge's `point`
   * event). */
  readonly pointable = $derived(this.entry !== null && this.context?.review != null);
  /** The plugin page's latest status line, as it reports it through the bridge. */
  status = $state<string | null>(null);

  async load(): Promise<void> {
    try {
      [this.entry, this.context] = await Promise.all([call(this.ipc.viewerEntry(this.session)), call(this.ipc.viewerContext(this.session))]);
      this.error = null;
    } catch (failure) {
      this.error = message(failure);
    }
    await Promise.all([this.announce(), this.readEval()]);
  }

  /** Read the context again: the active review changed. Pointing stops with the review it was for. */
  async refresh(): Promise<void> {
    try {
      this.context = await call(this.ipc.viewerContext(this.session));
    } catch (failure) {
      this.error = message(failure);
    }
    if (!this.pointable) this.pointing = false;
    await Promise.all([this.announce(), this.readEval()]);
  }

  /** Tell the other windows this viewer is open, for which Artifact and review. */
  async announce(open = true): Promise<void> {
    const presence: Presence = {
      session: this.session,
      artifact: this.context?.artifact ?? "",
      review: this.context?.review ?? null,
      open,
      pointable: this.pointable,
    };
    await emit(PRESENCE, presence).catch(() => {});
  }

  /** Append `text` to the active review's Notes through the library; the Notes it answers. The
   * windows showing those Notes are told what was pointed, which stands out there. */
  async append(text: string): Promise<string> {
    const notes = await call(this.ipc.viewerPoint(this.session, text));
    const review = this.context?.review;
    if (review) announcePointed(review, text);
    return notes;
  }

  /** Append `text`, a place pointed at, to the active review's draft; pointing ends. */
  async point(text: string): Promise<void> {
    this.pointing = false;
    try {
      await this.append(text);
      this.error = null;
    } catch (failure) {
      this.error = message(failure);
    }
  }

  /** The active review's eval, as the strip names it: read from the library's review. */
  async readEval(): Promise<void> {
    const review = this.context?.review;
    if (!review) {
      this.evalId = null;
      return;
    }
    try {
      const view = await call(this.ipc.humanOpen(review, false));
      if (this.context?.review === review) this.evalId = view.target.node.replace(/^e:/, "");
    } catch {
      // The strip names the review by its request until its eval can be read.
      this.evalId = null;
    }
  }
}

export const [getViewerState, setViewerState] = createContext<ViewerState>();

/** The caret's offset in the text under (`x`, `y`): where in a line a click landed. */
export function caretOffset(x: number, y: number): { node: Node; offset: number } | null {
  const doc = document as Document & {
    caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
  };
  const position = doc.caretPositionFromPoint?.(x, y);
  if (position) return { node: position.offsetNode, offset: position.offset };
  const range = doc.caretRangeFromPoint?.(x, y);
  return range ? { node: range.startContainer, offset: range.startOffset } : null;
}
