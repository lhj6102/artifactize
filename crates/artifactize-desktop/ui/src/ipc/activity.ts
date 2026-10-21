// The detached, read-only IPC observation contract shared with native test readers.
export type CommandActivity = Readonly<{
  revision: number;
  pending: number;
  commands: Readonly<Record<string, Readonly<{ started: number; completed: number; pending: number }>>>;
  search: Readonly<{ query: string; sequence: number; completed: boolean }> | null;
  /** E2E builds only (absent otherwise): manual refreshes that showed a fresh read, numbered
   * from 1; null before the first. */
  refresh?: Refresh | null;
}>;

export type Refresh = Readonly<{ generation: number }>;

// The observation a test build publishes on its window; tests read it through WebDriver.
declare global {
  interface Window {
    readonly artifactizeCommands: CommandActivity;
  }
}
