// The workbench backend's stand-in until its commands exist: a short made-up history for any
// worktree, worktree creation that names a sibling folder, and terminals that echo what is
// typed, with a line saying none of it is real. Frames arrive after the call returns, as on a
// Channel. Only backend.ts uses it.
import type { Commit, Created, TerminalFrame, Workbench, WorktreeKey } from "./backend";

const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });

const SUBJECTS = [
  "Merge pull request #439 from dev",
  "fix(desktop): keep the Detail's width within the graph",
  "docs: the workbench's layout",
  "feat(core): list every worktree",
  "test(ui): the camera between both panels",
  "chore: bump xterm",
  "refactor(core): one transport for hints",
  "fix(cli): verify waits for sign-offs",
];

function history(): Commit[] {
  const start = Date.UTC(2026, 10, 1, 12);
  return SUBJECTS.map((subject, at) => ({
    id: `${(0xbd0907a7 - at * 0x1f3).toString(16).padStart(8, "0")}${"0".repeat(32)}`,
    parents: at === SUBJECTS.length - 1 ? [] : [`${(0xbd0907a7 - (at + 1) * 0x1f3).toString(16).padStart(8, "0")}${"0".repeat(32)}`],
    refs: at === 0 ? ["HEAD", "main"] : at === 3 ? ["v0.19.1"] : [],
    subject,
    author: "Artifactize",
    time: start - at * 3_600_000,
  }));
}

export function fixture(): Workbench {
  const attached = new Map<WorktreeKey, (frame: TerminalFrame) => void>();
  /** Send `frame` to the worktree's attached screen, once the current call has returned. */
  const send = (worktree: WorktreeKey, frame: TerminalFrame) => queueMicrotask(() => attached.get(worktree)?.(frame));
  const lines = new Map<WorktreeKey, string>();
  let made = 0;
  const prompt = "\r\n$ ";
  return {
    gitHistory: () => ok({ commits: history(), head: history()[0].id, more: false }),
    worktreeCreate: (worktree) => {
      made += 1;
      return ok<Created>({ worktree: `${worktree}-${made}`, branch: `worktree-${made}` });
    },
    worktreeRemove: () => ok(null),
    terminalOpen: (worktree, _cols, _rows, frames) => {
      attached.set(worktree, frames);
      const typed = lines.get(worktree) ?? "";
      send(worktree, { kind: "snapshot", data: `\x1b[2mNo terminal host yet: this terminal only echoes.\x1b[0m${prompt}${typed}` });
      return ok(null);
    },
    terminalInput: (worktree, data) => {
      let typed = lines.get(worktree) ?? "";
      for (const key of data) {
        if (key === "\r") {
          send(worktree, { kind: "output", data: prompt });
          typed = "";
        } else if (key === "\x7f") {
          if (typed) send(worktree, { kind: "output", data: "\b \b" });
          typed = typed.slice(0, -1);
        } else if (key >= " ") {
          send(worktree, { kind: "output", data: key });
          typed += key;
        }
      }
      lines.set(worktree, typed);
      return ok(null);
    },
    terminalResize: () => ok(null),
    terminalClose: (worktree) => {
      attached.delete(worktree);
      return ok(null);
    },
    terminalRestore: () => ok(null),
  };
}
