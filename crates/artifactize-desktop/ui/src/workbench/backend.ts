// The workbench's door to the backend: the Git layer's history, worktree creation and removal,
// and each worktree's terminal, as the generated bindings type them. Only opening a terminal
// differs: its frames arrive on a Tauri Channel, which this door hides behind a callback.
import { Channel } from "@tauri-apps/api/core";
import type {
  Commands, CreatedWorktree_Serialize, Harness, RestoreOffer_Deserialize, ResumeSession, TerminalCommand_Deserialize, TerminalFrame_Deserialize,
} from "../ipc/client";

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
const HARNESS = { claudeCode: "Claude Code", codex: "Codex" } as const;

/** What resuming `session` of `harness` reads as: its label, "latest" unless the session is
 * known for certain, and the harness's own resume command. */
export function resumeOf(harness: Harness, session: ResumeSession): { label: string; command: string } {
  const exact = session.kind === "exact" ? session.id : null;
  const command =
    harness === "claudeCode" ? (exact ? `claude --resume ${exact}` : "claude --continue") : exact ? `codex resume ${exact}` : "codex resume --last";
  return { label: exact ? `Resume the ${HARNESS[harness]} session` : `Resume the latest ${HARNESS[harness]} session`, command };
}
