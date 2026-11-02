// The workbench's door to the backend: the Git layer's history, worktree creation and removal,
// and each worktree's terminal. It mirrors the tauri-specta commands `git_history`,
// `worktree_create`, `worktree_remove`, `terminal_open`, `terminal_input`, `terminal_resize`,
// `terminal_close` and `terminal_restore`, each answering as the generated bindings do, so
// `call` from ../ipc/client unwraps them. Until those bindings exist, `workbench` is the
// fixture (fixture.ts); swapping in the bindings changes only the last lines of this file.
import type { Failure, PathKey } from "../ipc/client";
import { fixture } from "./fixture";

/** A worktree's identity: its folder (a `BranchView`'s `workspace`), never its branch, so a
 * renamed or switched branch keeps its terminal. */
export type WorktreeKey = PathKey;

/** One commit of the Git layer. */
export type Commit = {
  id: string;
  /** Its parents' IDs, first parent first. */
  parents: string[];
  /** The refs pointing at it: `HEAD`, branch and tag names. */
  refs: string[];
  /** Its message's first line. */
  subject: string;
  author: string;
  /** When it was committed, in milliseconds since the epoch. */
  time: number;
};

/** A worktree's history, newest first, as the Git layer draws it. */
export type History = {
  commits: Commit[];
  /** The commit the worktree has checked out, if any. */
  head: string | null;
  /** Older commits exist than `commits` holds. */
  more: boolean;
};

/** A worktree artifactize created: a sibling folder of the primary checkout, on a generated
 * temporary branch. */
export type Created = { worktree: WorktreeKey; branch: string };

/** A known harness's resume command. `certain` when the remembered session ID is known for
 * certain (`claude --resume <id>`); otherwise `command` opens its picker or its latest session
 * (`claude --continue`, `codex resume --last`), and the offer says "latest session". */
export type Resume = { harness: string; command: string; certain: boolean };

/** What a terminal that was gone offers, once a fresh shell runs in its worktree. */
export type RestoreOffer = {
  /** The last directory, when it is known. */
  cwd: string | null;
  /** The last foreground command line, when it is known. */
  command: string | null;
  resume: Resume | null;
};

/** What the person confirmed of a restore offer. `fresh` keeps the fresh shell as it is. */
export type RestoreChoice = { kind: "fresh" } | { kind: "rerun" } | { kind: "resume" };

/** One frame of a terminal's stream, in order. A snapshot redraws the whole screen (cursor,
 * alternate screen and modes included) and comes first on every attach; output follows live. */
export type TerminalFrame =
  | { kind: "snapshot"; data: string }
  | { kind: "output"; data: string }
  /** The terminal's process ended; `code` is its exit status, when known. */
  | { kind: "ended"; code: number | null }
  /** The terminal was gone; a fresh shell runs, and this is what it can restore. */
  | { kind: "restore"; offer: RestoreOffer }
  /** The host could not detach from the app (Windows refused breakaway): the terminal ends
   * when the app does. */
  | { kind: "notPersistent"; reason: string }
  /** The agent in the terminal moved to another worktree. */
  | { kind: "agentMoved"; worktree: WorktreeKey };

type Typed<T> = { status: "ok"; data: T } | { status: "error"; error: Failure };

export interface Workbench {
  gitHistory(worktree: WorktreeKey): Promise<Typed<History>>;
  /** A new worktree checked out at `commit` of `worktree`'s repository. */
  worktreeCreate(worktree: WorktreeKey, commit: string): Promise<Typed<Created>>;
  /** Remove a worktree; its terminal ends. */
  worktreeRemove(worktree: WorktreeKey): Promise<Typed<null>>;
  /** Attach to the worktree's terminal, starting it the first time: frames arrive on `frames`
   * (a Tauri Channel in the bindings) until it is closed. */
  terminalOpen(worktree: WorktreeKey, cols: number, rows: number, frames: (frame: TerminalFrame) => void): Promise<Typed<null>>;
  terminalInput(worktree: WorktreeKey, data: string): Promise<Typed<null>>;
  terminalResize(worktree: WorktreeKey, cols: number, rows: number): Promise<Typed<null>>;
  /** Detach: the terminal and its processes keep running. */
  terminalClose(worktree: WorktreeKey): Promise<Typed<null>>;
  /** Run what the person confirmed of the restore offer. */
  terminalRestore(worktree: WorktreeKey, choice: RestoreChoice): Promise<Typed<null>>;
}

// With the generated bindings, this becomes:
//   import { Channel } from "@tauri-apps/api/core";
//   import { commands } from "../ipc/client";
//   export const workbench: Workbench = {
//     ...commands,  // gitHistory, worktreeCreate, ... as generated
//     terminalOpen: (worktree, cols, rows, frames) => {
//       const channel = new Channel<TerminalFrame>();
//       channel.onmessage = frames;
//       return commands.terminalOpen(worktree, cols, rows, channel);
//     },
//   };
export const workbench: Workbench = fixture();
