// The detached, read-only IPC observation contract shared with native test readers.
export type CommandActivity = Readonly<{
  revision: number;
  pending: number;
  commands: Readonly<Record<string, Readonly<{ started: number; completed: number; pending: number }>>>;
  search: Readonly<{ query: string; sequence: number; completed: boolean }> | null;
}>;
