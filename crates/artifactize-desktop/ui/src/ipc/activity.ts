// The detached, read-only IPC observation contract shared with native test readers.
export type SearchActivity = Readonly<{ query: string; sequence: number; completed: boolean; failed?: true }>;

export type CommandActivity = Readonly<{
  revision: number;
  pending: number;
  /** Per command: calls started, calls settled, those of them that rejected, and calls in flight. */
  commands: Readonly<Record<string, Readonly<{ started: number; completed: number; failed: number; pending: number }>>>;
  search: SearchActivity | null;
  reviewSearch: SearchActivity | null;
  /** E2E builds only (absent otherwise): manual refreshes that showed a fresh read, numbered
   * from 1; null before the first. */
  refresh?: Refresh | null;
}>;

export type Refresh = Readonly<{
  generation: number;
  /** Only public Pane labels and comments, from this reload's own reply. */
  branches: readonly Readonly<{ project: string; name: string; runs: readonly Readonly<{ id: string; comment: string | null }>[] }>[];
}>;

// The observation a test build publishes on its window; tests read it through WebDriver.
declare global {
  interface Window {
    readonly artifactizeCommands: CommandActivity;
  }
}
