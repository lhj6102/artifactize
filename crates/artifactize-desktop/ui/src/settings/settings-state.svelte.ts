import { createContext } from "svelte";
import {
  call,
  message,
  type CodexPrompt,
  type CodexStatus,
  type Commands,
  type CustomProvider,
  type EndpointApi,
  type KeyInput,
  type Model,
  type Provider,
  type Providers,
  type Pushed,
  type RemoteStatus,
  type Share,
  type SignoffGrace,
  type Target,
} from "../ipc/client";

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
  tab = $state<"providers" | "remote" | "limits">("providers");
  /** Numbers the remote-store calls: only the newest answer is shown. */
  remoteCalls = $state(0);
  /** Outstanding store reads or mutations, for the section's accessible busy state. */
  remotePending = $state(0);
  providers = $state.raw<Providers | null>(null);
  providersError = $state<string | null>(null);
  providersNotice = $state<string | null>(null);
  choice = $state.raw<Choice>({ kind: "builtin", provider: "codex" });
  /** The name `models` and an eval's `backend` use for the chosen provider. */
  backend = $derived(this.choice.kind === "builtin" ? this.choice.provider : this.choice.kind === "custom" ? this.choice.name : null);
  /** A built-in provider's key while typed; cleared the moment it is submitted. */
  key = $state("");
  draft = $state<Draft>(blank());
  saving = $state(false);
  codex = $state.raw<CodexStatus | null>(null);
  codexError = $state<string | null>(null);
  codexNotice = $state<string | null>(null);
  prompt = $state.raw<CodexPrompt | null>(null);
  redirect = $state("");
  busy = $state(false);
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

  readonly #commands: Commands;

  constructor(commands: Commands) {
    this.#commands = commands;
  }


  /** Read everything again: another process may have signed in, saved a key or changed a
   * custom provider. */
  async load() {
    await Promise.all([this.loadProviders(), this.loadCodex(), this.loadRemote(), this.loadGrace()]);
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
    this.graceNotice = null;
    try {
      this.#showGrace(await call(this.#commands.signoffGraceSet(this.graceSeconds)));
      this.graceNotice = "Saved. Runs started from now on wait this long.";
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
    this.models = null;
    this.modelsError = null;
    this.providersNotice = null;
    this.providersError = null;
    const custom = choice.kind === "custom" ? this.providers?.custom.find((one) => one.name === choice.name) : undefined;
    this.draft = custom ? draftOf(custom) : blank();
  }

  async saveKey(provider: Provider) {
    // The key leaves this state before the library is even asked.
    const typed = this.key;
    this.key = "";
    this.saving = true;
    this.providersNotice = null;
    this.providersError = null;
    try {
      await call(this.#commands.providerKeySave(provider, typed));
      this.providersNotice = "Saved the key.";
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
      await this.loadProviders();
    }
  }

  async removeKey(provider: Provider) {
    this.saving = true;
    this.providersNotice = null;
    this.providersError = null;
    try {
      await call(this.#commands.providerKeyRemove(provider));
      this.providersNotice = "Removed the saved key.";
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
    this.providersNotice = null;
    this.providersError = null;
    try {
      await call(this.#commands.providerCustomSave(name, target, key));
      await this.loadProviders();
      this.choose({ kind: "custom", name });
      this.providersNotice = `Saved ${name}.`;
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
    }
  }

  async removeCustom(name: string) {
    this.saving = true;
    this.providersNotice = null;
    this.providersError = null;
    try {
      await call(this.#commands.providerCustomRemove(name));
      await this.loadProviders();
      this.choose({ kind: "builtin", provider: "codex" });
      this.providersNotice = `Removed ${name}.`;
    } catch (failure) {
      this.providersError = message(failure);
    } finally {
      this.saving = false;
    }
  }

  async listModels() {
    const backend = this.backend;
    if (!backend) return;
    this.listing = true;
    this.models = null;
    this.modelsError = null;
    try {
      const models = await call(this.#commands.modelsList(backend));
      if (backend === this.backend) this.models = models;
    } catch (failure) {
      if (backend === this.backend) this.modelsError = message(failure);
    } finally {
      this.listing = false;
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

  async signIn() {
    this.codexError = null;
    this.codexNotice = null;
    this.busy = true;
    try {
      this.prompt = await call(this.#commands.codexLoginStart(true));
      this.codex = await call(this.#commands.codexLoginWait(this.prompt.signIn));
      this.codexNotice = "Signed in with Codex.";
    } catch (failure) {
      this.codexError = message(failure);
      await this.loadCodex();
    } finally {
      this.prompt = null;
      this.redirect = "";
      this.busy = false;
    }
  }

  async paste() {
    const text = this.redirect.trim();
    if (!text) return;
    try {
      await call(this.#commands.codexLoginPaste(text));
    } catch (failure) {
      this.codexError = message(failure);
    }
  }

  cancelSignIn() {
    void this.#commands.codexLoginCancel();
  }

  async signOut() {
    this.codexError = null;
    this.busy = true;
    try {
      const out = await call(this.#commands.codexLogout());
      this.codex = out.status;
      this.codexNotice = out.revoked ? "Signed out of Codex." : "Local tokens removed; their revocation was not confirmed.";
    } catch (failure) {
      this.codexError = message(failure);
    } finally {
      this.busy = false;
    }
  }

  /** Each remote call is numbered; only the newest one may show what it found, so a slow
   * status check never shows a store signed in after a sign-out. */
  async #remoteCall(run: () => Promise<RemoteStatus>, notice?: (status: RemoteStatus) => string) {
    const mine = ++this.remoteCalls;
    this.remotePending += 1;
    this.remoteError = null;
    try {
      const status = await run();
      if (mine !== this.remoteCalls) return;
      this.remote = status;
      this.remoteNotice = notice ? notice(status) : this.remoteNotice;
    } catch (failure) {
      if (mine === this.remoteCalls) this.remoteError = message(failure);
    } finally {
      this.remotePending -= 1;
    }
  }

  async loadRemote() {
    await this.#remoteCall(() => call(this.#commands.remoteStatus()));
  }

  async remoteLogin() {
    // The token leaves this state before the store is even asked.
    const typed = this.token;
    this.token = "";
    this.remoteNotice = null;
    this.working = true;
    await this.#remoteCall(
      () => call(this.#commands.remoteLogin(this.url.trim(), this.share, typed)),
      (status) =>
        status.state === "configured"
          ? `Signed in to ${status.url}${principalOf(status) ? ` as ${principalOf(status)}` : ""}.`
          : "Signed in.",
    );
    this.working = false;
  }

  async remoteLogout() {
    this.working = true;
    await this.#remoteCall(
      () => call(this.#commands.remoteLogout()),
      () => "Signed out of the remote review store. The server keeps the token valid until `artifactize server token revoke`.",
    );
    this.working = false;
  }

  async push(dryRun: boolean) {
    this.remoteError = null;
    this.working = true;
    try {
      const report: Pushed = await call(this.#commands.remotePush(dryRun));
      this.remoteNotice = `${dryRun ? "Would push" : "Pushed"} ${report.pushed}, already in the store ${report.existing}, skipped ${report.skipped}.`;
    } catch (failure) {
      this.remoteError = message(failure);
    } finally {
      this.working = false;
    }
  }
}

export const [getSettingsState, setSettingsState] = createContext<SettingsState>();
