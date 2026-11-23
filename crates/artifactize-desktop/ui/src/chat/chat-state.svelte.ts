// Chat mode's own facts: the sessions the library lists, the session shown with its transcript,
// the model chosen for its next turn, the Run reference to bring into view, the model menu's
// providers and what is typed in each composer. One owner, kept in the AppState; sessions, turns,
// Run cards, providers and models are the library's, read and changed through the IPC.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { GraphWorkspace } from "../monitor/graph-workspace.svelte";
import { call, message, type BranchKey, type ChatBranch, type ChatModel, type ChatSession, type ChatSessionView, type ChatView, type Commands } from "../ipc/client";
import { keyOf } from "../monitor/monitor-state.svelte";
import { providerEntries, readProvider, type MenuProvider } from "./model-providers";

class Composer {
  draft = $state("");
  shown = $derived(this.draft);
  #revision = 0;

  type(text: string): void {
    ++this.#revision;
    this.draft = text;
    this.shown = text;
  }

  begin(): (accepted: boolean) => void {
    const at = this.#revision;
    this.shown = "";
    return (accepted) => {
      if (at === this.#revision && (accepted || this.draft === "")) this.draft = "";
      this.shown = this.draft;
    };
  }
}

export class ChatModeState {
  composerPointer = $state(false);
  /** The library's sessions, newest activity first. */
  sessions = $state.raw<ChatSession[]>([]);
  query = $state("");
  /** The library has answered the current list query. */
  listed = $state(false);
  /** The session shown, by its ID. */
  selected = $state<string | null>(null);
  /** The shown session as the library last read it. */
  view = $state.raw<ChatView | null>(null);
  /** A session's inspection controls and where its conversation is read, kept independently. */
  readonly #graphs = new SvelteMap<string, GraphWorkspace>();
  readonly graph = $derived(this.selected === null ? null : this.#graphs.get(this.selected) ?? null);
  readonly scroll: Record<string, { top: number; following: boolean }> = $state({});
  /** A Run reference to bring into view once the shown session is read (J-RUN-ORIGIN). */
  reveal = $state<string | null>(null);
  /** What is typed in each session's composer, by session: kept while another is shown. */
  readonly #composers: Record<string, Composer> = Object.create(null);
  composer(id: string): Composer | undefined { return this.#composers[id]; }
  /** Work tied to a session, even if the person switches away while the command runs. */
  readonly sending = new SvelteMap<string, boolean>();
  readonly stopping = new SvelteMap<string, boolean>();
  readonly choosing = new SvelteMap<string, boolean>();
  readonly rejected = new SvelteMap<string, { text: string; reason: string }>();
  readonly #marking = new SvelteSet<string>();
  providers = $state.raw<MenuProvider[]>([]);
  providerError = $state<string | null>(null);
  modelQuery = $state("");
  /** The model a new session on a worktree starts with, as the library says: `known` once it
   * answered, `model` null when no provider lists one. */
  starting = $state.raw<{ worktree: string; branch: BranchKey | null; known: boolean; model: ChatModel | null } | null>(null);
  /** What went wrong, for the main area's notice. */
  error = $state<string | null>(null);
  noticeOpen = $state(false);
  /** The New chat dialog is open, and the worktree it would start the session in. */
  creating = $state(false);
  branches = $state.raw<ChatBranch[] | null>(null);
  branchError = $state<string | null>(null);
  worktree = $state<string | null>(null);
  /** The model menu is open. */
  picking = $state(false);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; search?: HTMLInputElement; main?: HTMLElement; composer?: HTMLTextAreaElement } = $state({});

  /** Counts reads of the shown session: only the newest answer is shown. */
  #reads = 0;
  #opening = 0;
  #views = 0;
  #lists = 0;
  #providerReads = 0;
  #defaults = 0;

  constructor(readonly ipc: Commands, readonly shown: () => boolean) {}

  search(query: string): void {
    if (query === this.query) return;
    this.query = query;
    this.listed = false;
    void this.list();
  }

  /** Read the library's match list; a late reply to an older search cannot replace it. */
  async list(): Promise<void> {
    const mine = ++this.#lists;
    try {
      const sessions = await call(this.ipc.chatList(null, this.query.trim() === "" ? null : this.query));
      if (mine !== this.#lists) return;
      this.sessions = sessions;
      this.listed = true;
    } catch (failure) {
      if (mine === this.#lists) this.error = message(failure);
    }
  }

  /** Show session `id`, optionally selecting a library-owned view before reading it. */
  async open(id: string, kind?: ChatSessionView, reveal: string | null = null): Promise<void> {
    const opening = ++this.#opening;
    if (kind) {
      try {
        await call(this.ipc.chatSetView(id, kind));
      } catch (failure) {
        if (opening === this.#opening) this.error = message(failure);
        return;
      }
    }
    if (opening !== this.#opening) return;
    ++this.#views;
    if (this.selected !== id) {
      this.view = null;
      this.picking = false;
    }
    this.reveal = reveal;
    this.#composers[id] ??= new Composer();
    this.selected = id;
    this.error = null;
    this.creating = false;
    if (!this.#graphs.has(id)) this.#graphs.set(id, new GraphWorkspace());
    await this.read();
  }

  /** A branch reference chooses the latest session in the library, not the visible list. */
  async openBranchGraph(branch: BranchKey, withoutSession: () => void): Promise<void> {
    const opening = ++this.#opening;
    try {
      const session = await call(this.ipc.chatLatestForBranch(branch));
      if (opening !== this.#opening) return;
      if (session) await this.open(session.id, "graph");
      else withoutSession();
    } catch (failure) {
      if (opening === this.#opening) this.error = message(failure);
    }
  }

  /** Change the shown session's view. The command's answer is the saved session. */
  async setView(kind: ChatSessionView): Promise<void> {
    const id = this.selected;
    if (!id || !this.view || this.view.session.view === kind) return;
    const changing = ++this.#views;
    try {
      const session = await call(this.ipc.chatSetView(id, kind));
      if (changing !== this.#views || this.view?.session.id !== id) return;
      // A read already in flight may have seen the old view. Read again for its transcript.
      this.view = { ...this.view, session };
      this.error = null;
      await this.read();
    } catch (failure) {
      if (changing === this.#views) this.error = message(failure);
    }
  }

  /** Open New chat on a branch from the library's full list, not Verify's search result. */
  async newChat(branch: BranchKey | null, exact: boolean): Promise<void> {
    const opening = ++this.#opening;
    this.branches = null;
    this.branchError = null;
    this.worktree = null;
    this.starting = null;
    this.creating = true;
    try {
      const branches = await call(this.ipc.chatBranches());
      if (!this.creating || opening !== this.#opening) return;
      this.branches = branches;
      const selected = (branch ? branches.find((item) => keyOf(item.key) === keyOf(branch)) : undefined) ?? (exact ? undefined : branches[0]);
      if (selected) {
        this.worktree = selected.workspace;
        void this.readStarting(selected.workspace, selected.key);
      }
    } catch (error) {
      if (this.creating && opening === this.#opening) this.branchError = message(error);
    }
  }

  /** Read the shown session again: its turns stream and its Run cards change. */
  async read(acknowledge = true): Promise<void> {
    const id = this.selected;
    if (id === null) return;
    const mine = ++this.#reads;
    try {
      const view = await call(this.ipc.chatOpen(id));
      if (mine !== this.#reads || this.selected !== id) return;
      this.view = view;
      if (!view.running) this.stopping.delete(id);
      if (acknowledge) this.acknowledge();
    } catch (failure) {
      if (mine === this.#reads && this.selected === id) this.error = message(failure);
    }
  }

  acknowledge(): void {
    if (this.view?.session.unseen && !this.view.running && this.shown()) void this.#markSeen(this.view.session.id);
  }

  async #markSeen(id: string): Promise<void> {
    if (this.#marking.has(id)) return;
    this.#marking.add(id);
    try {
      await call(this.ipc.chatMarkSeen(id));
      if (this.selected === id) await this.read(false);
      await this.list();
    } catch (failure) {
      if (this.selected === id) this.error = message(failure);
    } finally {
      this.#marking.delete(id);
    }
  }

  /** A pending message is neither in the transcript nor eligible to send twice. New text
   * stays independent of the pending submission. */
  async send(): Promise<void> {
    const view = this.view;
    if (!view || view.running || view.session.unavailable !== null || view.session.model === null) return;
    const id = view.session.id;
    const composer = this.composer(id);
    if (!composer) return;
    const text = composer.shown;
    if (text.trim() === "" || this.sending.has(id) || this.choosing.has(id) || this.stopping.has(id)) return;
    this.sending.set(id, true);
    this.rejected.delete(id);
    const finish = composer.begin();
    try {
      await call(this.ipc.chatSend(id, text, null));
      finish(true);
      if (this.selected === id) {
        this.error = null;
        await this.read();
      }
      void this.list();
    } catch (failure) {
      finish(false);
      this.rejected.set(id, { text, reason: message(failure) });
    } finally {
      this.sending.delete(id);
    }
  }

  restore(id: string): void {
    const rejected = this.rejected.get(id);
    const composer = this.composer(id);
    if (!rejected || !composer || composer.shown !== "") return;
    composer.type(rejected.text);
    this.rejected.delete(id);
  }

  async stop(): Promise<void> {
    const view = this.view;
    if (!view?.running || this.stopping.has(view.session.id)) return;
    const id = view.session.id;
    this.stopping.set(id, true);
    this.error = null;
    try {
      const requested = await call(this.ipc.chatStop(id));
      // True means cancellation was requested, not that the transcript has stopped yet.
      if (this.selected === id) await this.read();
      if (!requested) this.stopping.delete(id);
    } catch (failure) {
      this.stopping.delete(id);
      if (this.selected === id) this.error = message(failure);
    }
  }

  async openLink(url: string): Promise<void> {
    try {
      await call(this.ipc.chatOpenLink(url));
    } catch (failure) {
      this.error = message(failure);
    }
  }

  async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      this.error = "Clipboard is unavailable.";
    }
  }

  /** Save the selection now. For a model name, null tells the library to keep the prior
   * effort if the model offers it or use its default. */
  async setModel(id: string, selection: ChatModel): Promise<void> {
    this.picking = false;
    if (this.choosing.has(id)) return;
    this.choosing.set(id, true);
    try {
      const saved = await call(this.ipc.chatSetModel(id, selection));
      if (this.view?.session.id === id) {
        ++this.#reads; // An earlier in-flight read could still hold the previous selection.
        this.view = { ...this.view, session: saved };
        this.error = null;
        await this.read();
      }
    } catch (failure) {
      if (this.selected === id) this.error = message(failure);
    } finally {
      this.choosing.delete(id);
    }
  }

  /** Read the providers Settings lists and the models each offers, for the model menu. */
  async openPicker(): Promise<void> {
    this.picking = true;
    this.modelQuery = "";
    const mine = ++this.#providerReads;
    this.providerError = null;
    try {
      const listed = await call(this.ipc.providersList());
      if (mine !== this.#providerReads) return;
      const entries = providerEntries(listed);
      this.providers = entries.map((provider) => ({ provider, listing: { kind: "loading" } }));
      await Promise.all(entries.map(async (provider) => {
        const listing = await readProvider(this.ipc, provider);
        if (mine === this.#providerReads) {
          this.providers = this.providers.map((entry) => entry.provider === provider ? { provider, listing } : entry);
        }
      }));
    } catch (failure) {
      if (mine === this.#providerReads) this.providerError = message(failure);
    }
  }

  /** Ask the library which model a new session on `worktree` starts with; the dialog says it is
   * reading until it answers. */
  async readStarting(worktree: string, branch: BranchKey | null): Promise<void> {
    const mine = ++this.#defaults;
    this.starting = { worktree, branch, known: false, model: null };
    try {
      const model = await call(this.ipc.chatDefaultModel(worktree, branch));
      if (mine === this.#defaults) this.starting = { worktree, branch, known: true, model };
    } catch (failure) {
      if (mine !== this.#defaults) return;
      this.starting = { worktree, branch, known: true, model: null };
      this.error = message(failure);
    }
  }

  /** Start a session with the displayed default, or without a model when none was listed. */
  async create(worktree: string): Promise<boolean> {
    const starting = this.starting;
    if (starting?.worktree !== worktree || !starting.known) return false;
    try {
      const session = await call(this.ipc.chatCreate(worktree, starting.branch, starting.model));
      await this.list();
      await this.open(session.id);
      return true;
    } catch (failure) {
      this.error = message(failure);
      return false;
    }
  }
}
