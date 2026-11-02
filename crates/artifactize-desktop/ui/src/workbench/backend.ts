// The workbench's door to the backend: the Git layer's history, worktree creation and removal,
// and each worktree's terminal, as the generated bindings type them. Only opening a terminal
// differs: its frames arrive on a Tauri Channel, which this door hides behind a callback.
import { Channel } from "@tauri-apps/api/core";
import type { Commands, CreatedWorktree_Serialize, RestoreOffer_Deserialize, TerminalCommand_Deserialize, TerminalFrame_Deserialize } from "../ipc/client";

export type { Commit, RestoreChoice } from "../ipc/client";

/** A worktree's identity: its folder (a `BranchView`'s `workspace`), never its branch, so a
 * renamed or switched branch keeps its terminal. */
export type WorktreeKey = string;

/** A worktree artifactize created: a sibling folder of the primary checkout, on a generated
 * temporary branch. */
export type Created = CreatedWorktree_Serialize;
/** What a terminal that was gone offers, once a fresh shell runs in its worktree. */
export type RestoreOffer = RestoreOffer_Deserialize;
/** One frame of a terminal's stream, in order. A snapshot resets the screen and redraws it
 * whole at its size; output follows live. Bytes are raw terminal bytes. */
export type TerminalFrame = TerminalFrame_Deserialize;

type Door = Pick<Commands, "gitHistory" | "worktreeCreate" | "worktreeRemove" | "terminalInput" | "terminalResize" | "terminalClose" | "terminalRestore">;

export type Workbench = Door & {
  /** Attach to the worktree's terminal, starting it the first time; frames arrive on `frames`
   * until it is closed. */
  terminalOpen(worktree: WorktreeKey, cols: number, rows: number, frames: (frame: TerminalFrame) => void): ReturnType<Commands["terminalOpen"]>;
};

/** The workbench's commands of `commands` (the app's observed bindings). */
export function workbenchOf(commands: Commands): Workbench {
  const { gitHistory, worktreeCreate, worktreeRemove, terminalInput, terminalResize, terminalClose, terminalRestore } = commands;
  return {
    gitHistory,
    worktreeCreate,
    worktreeRemove,
    terminalInput,
    terminalResize,
    terminalClose,
    terminalRestore,
    terminalOpen: (worktree, cols, rows, frames) => commands.terminalOpen(worktree, cols, rows, new Channel<TerminalFrame>(frames)),
  };
}

/** An argv as a person would type it: arguments with spaces or quotes are quoted. */
export function commandLine(command: TerminalCommand_Deserialize): string {
  const quoted = (word: string) => (word === "" || /[\s'"\\$`]/.test(word) ? `'${word.replace(/'/g, `'\\''`)}'` : word);
  return [command.program, ...command.args].map(quoted).join(" ");
}

/** A harness as people call it. */
export const HARNESS = { claudeCode: "Claude Code", codex: "Codex" } as const;
