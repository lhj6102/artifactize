// One Artifact's own Human tools in its branch's worktree as it is now, and the Tool log of
// their runs while the app runs. A run needs no review and no claim; the backend runs it and
// sends what it prints keyed by the worktree and the Artifact. The worktree, not the branch, owns
// them: each call names the branch checked out in it now.
import type { Jobs } from "../ipc/jobs.svelte";
import { call, commands, message, type BranchKey, type Commands, type LogEntry, type Notice, type ToolView } from "../ipc/client";

/** The key of an Artifact's tools and log: its worktree as the backend names it, and its name. */
export const toolsKey = (workspace: string, artifact: string): string => `${workspace}\n${artifact}`;

type Busy = { label: string; job: number };

export class ArtifactTools {
  /** What the Artifact declares now, once read. */
  tools = $state.raw<ToolView[] | null>(null);
  error = $state<string | null>(null);
  busy = $state.raw<Busy | null>(null);
  notice = $state.raw<Notice | null>(null);
  /** Every run of its tools, oldest first. */
  log = $state.raw<LogEntry[]>([]);

  constructor(
    readonly artifact: string,
    readonly jobs: Jobs,
    readonly ipc: Commands = commands,
  ) {}

  /** Read what the Artifact declares in the worktree now, on `branch`. */
  async load(branch: BranchKey): Promise<void> {
    try {
      this.tools = await call(this.ipc.artifactTools(branch, this.artifact));
      this.error = null;
    } catch (error) {
      this.error = message(error);
    }
  }

  /** Run `tool` at once; a launch tool answers at once and its program keeps logging. */
  async run(branch: BranchKey, tool: ToolView): Promise<void> {
    if (this.busy) return;
    const job = this.jobs.next();
    this.busy = { label: `Running ${tool.operation}`, job };
    try {
      this.notice = await call(this.ipc.artifactToolRun(branch, this.artifact, tool.name, job));
    } catch (error) {
      this.notice = { text: message(error), error: true };
    } finally {
      if (this.busy?.job === job) this.busy = null;
    }
    // Declarations may have changed; show what the next run resolves.
    await this.load(branch);
  }

  cancel(): void {
    if (this.busy) void this.ipc.humanCancel(this.busy.job);
  }

  /** The Tool log, whole, as the backend sends it after each change. */
  logged(entries: LogEntry[]): void {
    this.log = entries;
  }
}
