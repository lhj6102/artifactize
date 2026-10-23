import { createContext } from "svelte";
import type { CodexPrompt, CodexStatus, KeyStatus, Model, Provider, RemoteStatus, Share, SignoffGrace } from "../ipc/client";

/** What the Settings screen holds: the snapshots it read, what is typed and what is under way. */
export class SettingsState {
  /** Numbers the remote-store calls: only the newest answer is shown. */
  remoteCalls = $state(0);
  /** Outstanding store reads or mutations, for the section's accessible busy state. */
  remotePending = $state(0);
  keys = $state.raw<KeyStatus[]>([]);
  codex = $state.raw<CodexStatus | null>(null);
  codexError = $state<string | null>(null);
  codexNotice = $state<string | null>(null);
  prompt = $state.raw<CodexPrompt | null>(null);
  redirect = $state("");
  busy = $state(false);
  provider = $state<Provider>("openai");
  models = $state.raw<Model[] | null>(null);
  modelsError = $state<string | null>(null);
  listing = $state(false);
  remote = $state.raw<RemoteStatus | null>(null);
  remoteError = $state<string | null>(null);
  remoteNotice = $state<string | null>(null);
  url = $state("");
  share = $state<Share>("summary");
  token = $state("");
  working = $state(false);
  /** The sign-off idle grace in force, as limits.json holds it. */
  grace = $state.raw<SignoffGrace | null>(null);
  /** The seconds typed for it; filled in with the one in force whenever that is read. */
  graceSeconds = $state<number | null>(null);
  graceError = $state<string | null>(null);
  graceNotice = $state<string | null>(null);
  graceSaving = $state(false);
}

export const [getSettingsState, setSettingsState] = createContext<SettingsState>();
