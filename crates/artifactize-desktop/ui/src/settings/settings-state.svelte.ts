import { createContext } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import {
  call,
  message,
  type AntigravityPrompt,
  type AntigravityStatus,
  type ChatAgentAvailability,
  type ChatAgentCatalogEntry,
  type ChatRegistryEntry,
  type ChatRegistryInstallSpec,
  type ChatTrustRecord,
  type ChatTrustScope,
  type ClaudeAgentPrompt,
  type ClaudeAgentStatus,
  type CodexPrompt,
  type CodexStatus,
  type Commands,
  type CustomProvider,
  type EndpointApi,
  type KeyInput,
  type ModelsResult,
  type Provider,
  type Providers,
  type Pushed,
  type RemoteStatus,
  type Share,
  type SignoffGrace,
  type Target,
} from "../ipc/client";

/** Which item is shown in the Chat Agents tab. */
export type ChatAgentChoice =
  | { kind: "agent"; id: string }
  | { kind: "registry" }
  | { kind: "new" };

/** Draft form for a machine-local custom ACP Chat agent; secret values are write-only. */
export type CustomAgentDraft = {
  id: string;
  name: string;
  command: string;
  argsText: string;
  envText: string;
  secretKey: string;
  secretValue: string;
  savedSecretKeys: string[];
  removeSecretKeys: string[];
  mcpStdio: boolean;
  editing: boolean;
};

const blankCustomAgent = (): CustomAgentDraft => ({
  id: "",
  name: "",
  command: "",
  argsText: "",
  envText: "",
  secretKey: "",
  secretValue: "",
  savedSecretKeys: [],
  removeSecretKeys: [],
  mcpStdio: true,
  editing: false,
});

function customAgentDraftOf(entry: ChatAgentCatalogEntry): CustomAgentDraft {
  const envText = Object.entries(entry.env)
    .map(([k, v]) => `${k}=${v ?? ""}`)
    .join("\n");
  return {
    id: entry.id,
    name: entry.name,
    command: entry.command,
    argsText: entry.args.join("\n"),
    envText,
    secretKey: "",
    secretValue: "",
    savedSecretKeys: [...entry.secretEnvKeys],
    removeSecretKeys: [],
    mcpStdio: entry.mcpStdio,
    editing: true,
  };
}

/** The provider whose setup the Providers tab shows: a built-in one, a custom one by name, or
 * a new custom provider being written. */
export type Choice = { kind: "builtin"; provider: Provider } | { kind: "custom"; name: string } | { kind: "new" };

/** A custom provider as its form holds it. The key is only ever typed here, never read back. */
export type Draft = {
  name: string;
  kind: "alias" | "endpoint";
  provider: Provider;
  api: EndpointApi;
  url: string;
  key: string;
  /** The endpoint has a saved key, which an empty key field keeps. */
  saved: boolean;
  /** Save the endpoint without a key, dropping a saved one. */
  noKey: boolean;
};

const blank = (): Draft => ({
  name: "",
  kind: "endpoint",
  provider: "codex",
  api: "openaiChat",
  url: "",
  key: "",
  saved: false,
  noKey: false,
});

function draftOf(custom: CustomProvider): Draft {
  const target = custom.target;
  return target.kind === "alias"
    ? { ...blank(), name: custom.name, kind: "alias", provider: target.provider }
    : { ...blank(), name: custom.name, api: target.api, url: target.url, saved: target.key ?? false };
}

/** The principal the store named, when it was reachable and named one. */
export const principalOf = (status: RemoteStatus | null) =>
  status?.state === "configured" && status.connection.state === "signedIn" ? status.connection.principal : null;

/** What the Settings modal holds: the snapshots it read, what is typed and what is under way,
 * and the library commands that change them. */
export class SettingsState {
  /** Numbers the remote-store calls: only the newest answer is shown. */
  remoteCalls = $state(0);
  /** Outstanding store reads or mutations, for the section's accessible busy state. */
  remotePending = $state(0);
  providers = $state.raw<Providers | null>(null);
  providersError = $state<string | null>(null);
  choice = $state.raw<Choice>({ kind: "builtin", provider: "codex" });
  /** The name `models` and an eval's `backend` use for the chosen provider. */
  backend = $derived(this.choice.kind === "builtin" ? this.choice.provider : this.choice.kind === "custom" ? this.choice.name : null);
  /** A built-in provider's key while typed; cleared the moment it is submitted. */
  key = $state("");
  draft = $state<Draft>(blank());
  saving = $state(false);
  codex = $state.raw<CodexStatus | null>(null);
  codexError = $state<string | null>(null);
  /** A sign-out whose revocation the provider did not confirm: said as a warning. */
  codexWarning = $state<string | null>(null);
  prompt = $state.raw<CodexPrompt | null>(null);
  /** What finishes a sign-in by hand is unfolded; every sign-in starts with it folded. */
  byHand = $state(false);
  redirect = $state("");
  busy = $state(false);
  /** Claude Code's sign-in as Claude Code last reported it; asked whenever Claude Agent is
   * chosen. */
  claude = $state.raw<ClaudeAgentStatus | null>(null);
  claudeError = $state<string | null>(null);
  claudePrompt = $state.raw<ClaudeAgentPrompt | null>(null);
  /** The code Claude Code's sign-in page gives, while typed. */
  claudeCode = $state("");
  /** Claude Code is being asked, signed in or signed out. */
  claudeBusy = $state(false);
  /** Antigravity's own sign-in, read when its setup is chosen under Chat Agents. */
  antigravity = $state.raw<AntigravityStatus | null>(null);
  antigravityError = $state<string | null>(null);
  antigravityAttempt = $state<number | null>(null);
  antigravityPrompt = $state.raw<AntigravityPrompt | null>(null);
  antigravityByHand = $state(false);
  antigravityCode = $state("");
  antigravityBusy = $state(false);
  /** Chat Agents tab: 3-tier ACP catalog, probe results, Registry, custom draft, and Workspace Trust. */
  chatAgents = $state.raw<ChatAgentCatalogEntry[]>([]);
  chatAgentsError = $state<string | null>(null);
  chatAgentChoice = $state.raw<ChatAgentChoice>({ kind: "agent", id: "antigravity" });
  readonly chatAgentProbes = new SvelteMap<string, ChatAgentAvailability | { kind: "loading" }>();
  trustRecords = $state.raw<ChatTrustRecord[]>([]);
  registryEntries = $state.raw<ChatRegistryEntry[] | null>(null);
  registryLoading = $state(false);
  registryError = $state<string | null>(null);
  registryConfirm = $state.raw<ChatRegistryInstallSpec | null>(null);
  registryInstalling = $state(false);
  customAgentDraft = $state<CustomAgentDraft>(blankCustomAgent());
  customAgentSaving = $state(false);
  #modelReads = 0;
  models = $state.raw<ModelsResult | null>(null);
  listing = $state(false);
  remote = $state.raw<RemoteStatus | null>(null);
  remoteError = $state<string | null>(null);
  remoteNotice = $state.raw<{ text: string; dryRun: boolean } | null>(null);
  /** When the remote status was last read, in milliseconds since 1970: what was pushed today
   * counts from the local midnight before it. */
  url = $state("");
  share = $state<Share>("summary");
  token = $state("");
  working = $state(false);
  /** The sign-off idle grace in force, as limits.json holds it. */
  grace = $state.raw<SignoffGrace | null>(null);
  /** The seconds typed for it; filled in with the one in force whenever that is read. */
  graceSeconds = $state<number | null>(null);
  graceError = $state<string | null>(null);
  graceSaving = $state(false);

  readonly #commands: Commands;

  constructor(commands: Commands) {
    this.#commands = commands;
  }


  /** Read everything again: another process may have signed in, saved a key or changed a
   * custom provider. */
  async load() {
    await Promise.all([
      this.loadProviders(),
      this.loadChatAgents(),
      this.loadCodex(),
      this.loadRemote(),
      this.loadGrace(),
    ]);
  }

  async loadChatAgents() {
    try {
      const [agents, trust] = await Promise.all([
        call(this.#commands.chatAgentsList()),
        call(this.#commands.chatTrustList()),
      ]);
      this.chatAgents = agents;
      this.trustRecords = trust;
      this.chatAgentsError = null;
      if (this.chatAgentChoice.kind === "agent") {
        const selected = this.chatAgentChoice.id;
        if (!agents.some((a) => a.id === selected) && agents.length > 0) {
          this.chatAgentChoice = { kind: "agent", id: agents[0].id };
        }
      }
    } catch (failure) {
      this.chatAgentsError = message(failure);
    }
  }

  chooseChatAgent(choice: ChatAgentChoice) {
    this.chatAgentChoice = choice;
    this.chatAgentsError = null;
    if (choice.kind === "new") {
      this.customAgentDraft = blankCustomAgent();
    } else if (choice.kind === "registry") {
      if (this.registryEntries === null && !this.registryLoading) void this.loadRegistry();
    } else if (choice.kind === "agent") {
      if (choice.id === "antigravity") void this.loadAntigravity();
      if (!this.chatAgentProbes.has(choice.id)) void this.probeChatAgent(choice.id, false);
    }
  }

  editCustomAgent(entry: ChatAgentCatalogEntry) {
    this.customAgentDraft = customAgentDraftOf(entry);
    this.chatAgentChoice = { kind: "new" };
    this.chatAgentsError = null;
  }

  async probeChatAgent(id: string, refresh = false) {
    this.chatAgentProbes.set(id, { kind: "loading" });
    try {
      const result = await call(this.#commands.chatAgentProbe(id, refresh));
      this.chatAgentProbes.set(id, result);
    } catch (failure) {
      this.chatAgentProbes.set(id, { kind: "failed", executable: null, version: null, error: message(failure) });
    }
  }

  async revokeTrust(agentId: string, scope: ChatTrustScope) {
    this.chatAgentsError = null;
    try {
      await call(this.#commands.chatTrustRevoke(agentId, scope));
      this.trustRecords = await call(this.#commands.chatTrustList());
    } catch (failure) {
      this.chatAgentsError = message(failure);
    }
  }

  async loadRegistry() {
    this.registryLoading = true;
    this.registryError = null;
    try {
      this.registryEntries = await call(this.#commands.chatAgentRegistryList());
    } catch (failure) {
      this.registryError = message(failure);
    } finally {
      this.registryLoading = false;
    }
  }

  async installRegistry(spec: ChatRegistryInstallSpec) {
    this.registryInstalling = true;
    this.registryError = null;
    try {
      const installed = await call(this.#commands.chatAgentRegistryInstall(spec));
      this.registryConfirm = null;
      await this.loadChatAgents();
      this.chooseChatAgent({ kind: "agent", id: installed.id });
    } catch (failure) {
      this.registryError = message(failure);
    } finally {
      this.registryInstalling = false;
    }
  }

  async saveCustomAgent() {
    const draft = this.customAgentDraft;
    const args = draft.argsText
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    const env: Record<string, string> = {};
    for (const raw of draft.envText.split("\n")) {
      const line = raw.trim();
      if (!line) continue;
      const eq = line.indexOf("=");
      if (eq <= 0) {
        this.chatAgentsError = `Environment line "${line}" must use KEY=VALUE syntax.`;
        return;
      }
      env[line.slice(0, eq).trim()] = line.slice(eq + 1);
    }
    const setSecrets: Record<string, string> = {};
    const secretKey = draft.secretKey.trim();
    const secretValue = draft.secretValue;
    // Secret value leaves frontend state before the IPC call starts.
    this.customAgentDraft.secretKey = "";
    this.customAgentDraft.secretValue = "";
    if (secretKey) {
      setSecrets[secretKey] = secretValue;
    }
    this.customAgentSaving = true;
    this.chatAgentsError = null;
    try {
      const saved = await call(
        this.#commands.chatAgentCustomSave({
          id: draft.id.trim(),
          name: draft.name.trim() || draft.id.trim(),
          command: draft.command.trim(),
          args,
          env,
          setSecrets,
          removeSecretKeys: draft.removeSecretKeys,
          mcpStdio: draft.mcpStdio,
        }),
      );
      await this.loadChatAgents();
      this.chooseChatAgent({ kind: "agent", id: saved.id });
    } catch (failure) {
      this.chatAgentsError = message(failure);
    } finally {
      this.customAgentSaving = false;
    }
  }

  async removeCustomAgent(id: string) {
    this.customAgentSaving = true;
    this.chatAgentsError = null;
    try {
      await call(this.#commands.chatAgentCustomRemove(id));
      this.chatAgentProbes.delete(id);
      await this.loadChatAgents();
      this.chooseChatAgent({ kind: "agent", id: this.chatAgents[0]?.id ?? "antigravity" });
    } catch (failure) {
      this.chatAgentsError = message(failure);
    } finally {
      this.customAgentSaving = false;
    }
  }

  /** The grace in force, and the seconds field set to it. */
  #showGrace(grace: SignoffGrace) {
    this.grace = grace;
    this.graceSeconds = grace.seconds;
  }

  async loadGrace() {
    try {
      this.#showGrace(await call(this.#commands.signoffGrace()));
      this.graceError = null;
    } catch (failure) {
      this.graceError = message(failure);
    }
  }

  async saveGrace() {
    // Whether the number is a grace the library accepts is the library's answer, shown on Save.
    if (this.graceSeconds === null) return;
    this.graceSaving = true;
    this.graceError = null;
    try {
      this.#showGrace(await call(this.#commands.signoffGraceSet(this.graceSeconds)));
    } catch (failure) {
      this.graceError = message(failure);
    } finally {
      this.graceSaving = false;
    }
  }

  async loadProviders() {
    try {
      this.providers = await call(this.#commands.providersList());
      this.providersError = null;
    } catch (failure) {
      this.providersError = message(failure);
    }
  }

  /** Show `choice`'s setup; a custom provider's form starts from what is saved. */
  choose(choice: Choice) {
    this.choice = choice;
    this.key = "";
    this.#clearModels();
    this.providersError = null;
    const custom = choice.kind === "custom" ? this.providers?.custom.find((one) => one.name === choice.name) : undefined;
    this.draft = custom ? draftOf(custom) : blank();
    if (choice.kind === "builtin" && choice.provider === "claude-agent" && !this.claudeBusy) void this.loadClaude();
  }

  async saveKey(provider: Provider) {
    // The key leaves this state before the library is even asked.
    const typed = this.key;
    this.key = "";
    this.saving = true;
    this.providersError = null;
    try {
      await call(this.#commands.providerKeySave(provider, typed));
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
      await this.loadProviders();
    }
  }

  async removeKey(provider: Provider) {
    this.saving = true;
    this.providersError = null;
    try {
      await call(this.#commands.providerKeyRemove(provider));
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
      await this.loadProviders();
    }
  }

  async saveCustom() {
    const draft = this.draft;
    const target: Target =
      draft.kind === "alias"
        ? { kind: "alias", provider: draft.provider }
        : { kind: "endpoint", api: draft.api, url: draft.url.trim() };
    const key: KeyInput =
      draft.kind === "alias" || draft.noKey
        ? { kind: "none" }
        : draft.key
          ? { kind: "set", key: draft.key }
          : draft.saved
            ? { kind: "keep" }
            : { kind: "none" };
    const name = draft.name.trim();
    // The key leaves this state before the library is even asked.
    this.draft.key = "";
    this.saving = true;
    this.providersError = null;
    try {
      await call(this.#commands.providerCustomSave(name, target, key));
      await this.loadProviders();
      this.choose({ kind: "custom", name });
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
    }
  }

  async removeCustom(name: string) {
    this.saving = true;
    this.providersError = null;
    try {
      await call(this.#commands.providerCustomRemove(name));
      await this.loadProviders();
      this.choose({ kind: "builtin", provider: "codex" });
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
    }
  }

  #clearModels() {
    ++this.#modelReads;
    this.models = null;
    this.listing = false;
  }

  async listModels() {
    const backend = this.backend;
    if (!backend) return;
    this.#clearModels();
    const read = this.#modelReads;
    this.listing = true;
    try {
      const models = await call(this.#commands.modelsList(backend));
      if (read === this.#modelReads) this.models = models;
    } catch (failure) {
      if (read === this.#modelReads) this.models = { kind: "failed", error: message(failure) };
    } finally {
      if (read === this.#modelReads) this.listing = false;
    }
  }

  async loadCodex() {
    try {
      this.codex = await call(this.#commands.codexStatus());
      this.codexError = null;
    } catch (failure) {
      this.codexError = message(failure);
    }
  }

  codexAttempt = $state<number | null>(null);
  claudeAttempt = $state<number | null>(null);

  async signIn() {
    this.codexError = null;
    this.codexWarning = null;
    this.byHand = false;
    this.busy = true;
    try {
      this.codexAttempt = await call(this.#commands.codexLoginStart(true));
      this.prompt = await call(this.#commands.codexLoginPrompt(this.codexAttempt));
      this.codex = await call(this.#commands.codexLoginWait(this.prompt.signIn));
    } catch (failure) {
      await this.loadCodex();
      const error = message(failure);
      if (error !== "Codex sign-in was cancelled." || this.codex?.state === "absent") this.codexError = error;
    } finally {
      this.prompt = null;
      this.codexAttempt = null;
      this.redirect = "";
      this.busy = false;
    }
  }

  async paste() {
    const text = this.redirect.trim();
    if (!text || !this.prompt) return;
    try {
      await call(this.#commands.codexLoginPaste(this.prompt.signIn, text));
    } catch (failure) {
      this.codexError = message(failure);
    }
  }

  cancelSignIn() {
    if (this.codexAttempt !== null) void this.#commands.codexLoginCancel(this.codexAttempt);
  }

  async signOut() {
    this.codexError = null;
    this.codexWarning = null;
    this.busy = true;
    try {
      const out = await call(this.#commands.codexLogout());
      this.codex = out.status;
      if (!out.revoked) this.codexWarning = "Local tokens removed; their revocation was not confirmed.";
    } catch (failure) {
      this.codexError = message(failure);
    } finally {
      this.busy = false;
    }
  }

  /** Ask Claude Code again whether it is signed in. */
  async loadClaude() {
    this.claudeBusy = true;
    this.claudeError = null;
    try {
      this.claude = await call(this.#commands.claudeAgentStatus());
    } catch (failure) {
      this.claudeError = message(failure);
    } finally {
      this.claudeBusy = false;
    }
  }

  /** Run Claude Code's own sign-in; its status shows again when it ends. */
  async claudeSignIn() {
    this.claudeError = null;
    this.byHand = false;
    this.claudeBusy = true;
    try {
      this.claudeAttempt = await call(this.#commands.claudeAgentLoginStart());
      const started = await call(this.#commands.claudeAgentLoginPrompt(this.claudeAttempt));
      if ("signIn" in started) {
        this.claudePrompt = started;
        this.claude = await call(this.#commands.claudeAgentLoginWait(started.signIn));
      } else {
        this.claude = started;
      }
    } catch (failure) {
      this.claudeError = message(failure);
    } finally {
      this.claudePrompt = null;
      this.claudeAttempt = null;
      this.claudeCode = "";
      this.claudeBusy = false;
    }
    if (this.claudeError !== null) await this.#refreshClaude();
  }

  /** The status after a failed or cancelled sign-in, keeping the error shown. */
  async #refreshClaude() {
    try {
      this.claude = await call(this.#commands.claudeAgentStatus());
    } catch {
      // The sign-in's own error stays shown.
    }
  }

  async claudePaste() {
    const code = this.claudeCode.trim();
    if (!code || !this.claudePrompt) return;
    try {
      await call(this.#commands.claudeAgentLoginPaste(this.claudePrompt.signIn, code));
    } catch (failure) {
      this.claudeError = message(failure);
    }
  }

  cancelClaudeSignIn() {
    if (this.claudeAttempt !== null) void this.#commands.claudeAgentLoginCancel(this.claudeAttempt);
  }

  async claudeSignOut() {
    this.claudeError = null;
    this.claudeBusy = true;
    try {
      this.claude = await call(this.#commands.claudeAgentLogout());
    } catch (failure) {
      this.claudeError = message(failure);
    } finally {
      this.claudeBusy = false;
    }
  }

  #showAntigravity(status: AntigravityStatus) {
    if (this.antigravity && this.antigravity.state !== status.state) {
      this.chatAgentProbes.delete("antigravity");
    }
    this.antigravity = status;
  }

  /** Ask the Antigravity CLI again; keep its result separate from a sign-in or sign-out error. */
  async #readAntigravity() {
    this.#showAntigravity(await call(this.#commands.antigravityStatus()));
  }

  async loadAntigravity() {
    if (this.antigravityBusy) return;
    this.antigravityBusy = true;
    this.antigravityError = null;
    try {
      await this.#readAntigravity();
    } catch (failure) {
      this.antigravityError = message(failure);
    } finally {
      this.antigravityBusy = false;
    }
  }

  async antigravitySignIn() {
    if (this.antigravityBusy) return;
    this.antigravityBusy = true;
    this.antigravityError = null;
    this.antigravityByHand = false;
    try {
      this.antigravityAttempt = await call(this.#commands.antigravityLoginStart());
      const started = await call(this.#commands.antigravityLoginPrompt(this.antigravityAttempt));
      if ("signIn" in started) {
        this.antigravityPrompt = started;
        // A browser that cannot open must not stop Finish by hand or hold up the wait.
        void call(this.#commands.chatOpenLink(started.url)).catch((failure: unknown) => {
          if (this.antigravityPrompt === started) this.antigravityError = message(failure);
        });
        this.#showAntigravity(await call(this.#commands.antigravityLoginWait(started.signIn)));
      } else {
        this.#showAntigravity(started);
      }
      this.antigravityError = null;
    } catch (failure) {
      this.antigravityError = message(failure);
      this.antigravityPrompt = null;
      this.antigravityAttempt = null;
      // Even if sign-in failed or was cancelled, the CLI may now report another status.
      await this.#readAntigravity().catch(() => {});
    } finally {
      this.antigravityPrompt = null;
      this.antigravityAttempt = null;
      this.antigravityCode = "";
      this.antigravityBusy = false;
    }
  }

  async antigravityPaste() {
    const code = this.antigravityCode.trim();
    const prompt = this.antigravityPrompt;
    if (!code || !prompt) return;
    this.antigravityCode = "";
    this.antigravityError = null;
    try {
      await call(this.#commands.antigravityLoginPaste(prompt.signIn, code));
    } catch (failure) {
      if (this.antigravityPrompt === prompt) this.antigravityError = message(failure);
    }
  }

  async cancelAntigravitySignIn() {
    const attempt = this.antigravityAttempt;
    if (attempt === null) return;
    try {
      await this.#commands.antigravityLoginCancel(attempt);
    } catch (failure) {
      if (this.antigravityAttempt === attempt) this.antigravityError = message(failure);
    }
  }

  async antigravitySignOut() {
    this.antigravityError = null;
    this.antigravityBusy = true;
    try {
      this.#showAntigravity(await call(this.#commands.antigravityLogout()));
    } catch (failure) {
      this.antigravityError = message(failure);
    } finally {
      this.antigravityBusy = false;
    }
  }

  /** Each remote call is numbered; only the newest one may show what it found, so a slow
   * status check never shows a store signed in after a sign-out. */
  async #remoteCall(run: () => Promise<RemoteStatus>) {
    const mine = ++this.remoteCalls;
    this.remotePending += 1;
    this.remoteError = null;
    try {
      const status = await run();
      if (mine !== this.remoteCalls) return;
      this.remote = status;
    } catch (failure) {
      if (mine === this.remoteCalls) this.remoteError = message(failure);
    } finally {
      this.remotePending -= 1;
    }
  }

  /** Read the store's status again; a push's result gives way to it. */
  async loadRemote() {
    this.remoteNotice = null;
    await this.#remoteCall(() => call(this.#commands.remoteStatus()));
  }

  async remoteLogin() {
    // The token leaves this state before the store is even asked.
    const typed = this.token;
    this.token = "";
    this.remoteNotice = null;
    this.working = true;
    await this.#remoteCall(() => call(this.#commands.remoteLogin(this.url.trim(), this.share, typed)));
    this.working = false;
  }

  async remoteLogout() {
    this.remoteNotice = null;
    this.working = true;
    await this.#remoteCall(() => call(this.#commands.remoteLogout()));
    this.working = false;
  }

  async push(dryRun: boolean) {
    this.remoteError = null;
    this.working = true;
    try {
      const report: Pushed = await call(this.#commands.remotePush(dryRun));
      this.remoteNotice = { dryRun, text: `${dryRun ? "Would push" : "Pushed"} ${report.pushed}, already in the store ${report.existing}, skipped ${report.skipped}.` };
      // What was pushed today counts this push from now on; its result stays the status line.
      if (!dryRun) await this.#remoteCall(() => call(this.#commands.remoteStatus()));
    } catch (failure) {
      this.remoteError = message(failure);
    } finally {
      this.working = false;
    }
  }
}

export const [getSettingsState, setSettingsState] = createContext<SettingsState>();
