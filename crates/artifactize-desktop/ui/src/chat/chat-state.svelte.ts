// Chat mode's own facts: the sessions the library lists, the session shown with its transcript,
// the model chosen for its next turn, the Run reference to bring into view, the model menu's
// providers and what is typed in each composer. One owner, kept in the AppState; sessions, turns,
// Run cards, providers and models are the library's, read and changed through the IPC.
import { SvelteMap } from "svelte/reactivity";
import { GraphWorkspace } from "../monitor/graph-workspace.svelte";
import { call, message, type BranchKey, type ChatModel, type ChatSession, type ChatSessionView, type ChatView, type Commands, type Model } from "../ipc/client";

/** A provider of the model menu: its models once listed, or why it lists none. */
export type MenuProvider = { provider: string; models: Model[] | null; error: string | null };

export class ChatModeState {
  /** The library's sessions, newest activity first. */
  sessions = $state.raw<ChatSession[]>([]);
  /** The library has answered the list at least once. */
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
  readonly typed: Record<string, string> = $state({});
  /** The model chosen for each session's next turn, by session, until it is sent. */
  readonly chosen: Record<string, ChatModel> = $state({});
  /** The providers Settings lists, with their models, as the model menu shows them. */
  providers = $state.raw<MenuProvider[]>([]);
  /** The model a new session on a worktree starts with, as the library says: `known` once it
   * answered, `model` null when no provider lists one. */
  starting = $state.raw<{ worktree: string; known: boolean; model: ChatModel | null } | null>(null);
  /** What went wrong, for the main area's notice. */
  error = $state<string | null>(null);
  noticeOpen = $state(false);
  /** The New chat dialog is open, and the worktree it would start the session in. */
  creating = $state(false);
  worktree = $state<string | null>(null);
  /** The model menu is open. */
  picking = $state(false);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; composer?: HTMLTextAreaElement } = $state({});

  /** Counts reads of the shown session: only the newest answer is shown. */
  #reads = 0;
  #opening = 0;
  #views = 0;

  constructor(readonly ipc: Commands) {}

  /** The model the shown session's next turn runs with. */
  model(session: ChatSession): ChatModel {
    return this.chosen[session.id] ?? session.selection;
  }

  /** Read the sessions again. */
  async list(): Promise<void> {
    try {
      this.sessions = await call(this.ipc.chatList(null, null));
      this.listed = true;
    } catch (failure) {
      this.error = message(failure);
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
    if (this.selected !== id) this.view = null;
    this.reveal = reveal;
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

  /** Open the new session dialog on `worktree` and read its library default. */
  newChat(worktree: string | null): void {
    ++this.#opening;
    this.worktree = worktree;
    this.creating = true;
    if (worktree) void this.readStarting(worktree);
  }

  /** Read the shown session again: its turns stream and its Run cards change. */
  async read(): Promise<void> {
    const id = this.selected;
    if (id === null) return;
    const mine = ++this.#reads;
    try {
      const view = await call(this.ipc.chatOpen(id));
      if (mine === this.#reads && this.selected === id) this.view = view;
    } catch (failure) {
      if (mine === this.#reads) this.error = message(failure);
    }
  }

  /** Send what is typed in the shown session, with the model chosen for it if one was. */
  async send(): Promise<void> {
    const view = this.view;
    if (!view || view.running || view.session.unavailable !== null) return;
    const id = view.session.id;
    const draft = this.typed[id] ?? "";
    const text = draft.trim();
    if (text === "") return;
    const chosen = this.chosen[id] ?? null;
    // The composer empties for the next message; the draft comes back if nothing was sent.
    this.typed[id] = "";
    try {
      await call(this.ipc.chatSend(id, text, chosen));
      delete this.chosen[id];
      this.error = null;
      await this.read();
    } catch (failure) {
      // Nothing was sent: the draft comes back as it was, unless something new was typed since.
      if ((this.typed[id] ?? "") === "") this.typed[id] = draft;
      this.error = message(failure);
    }
  }

  /** Stop the shown session's running turn. */
  async stop(): Promise<void> {
    const id = this.view?.session.id;
    if (!id) return;
    try {
      await call(this.ipc.chatStop(id));
      await this.read();
    } catch (failure) {
      this.error = message(failure);
    }
  }

  /** Read the providers Settings lists and the models each offers, for the model menu. */
  async providersRead(): Promise<void> {
    try {
      const listed = await call(this.ipc.providersList());
      const names = [...listed.builtin, ...listed.custom.map((custom) => custom.name)];
      this.providers = names.map((provider) => ({ provider, models: null, error: null }));
      await Promise.all(
        names.map(async (provider) => {
          let entry: MenuProvider;
          try {
            entry = { provider, models: await call(this.ipc.modelsList(provider)), error: null };
          } catch (failure) {
            entry = { provider, models: null, error: message(failure) };
          }
          this.providers = this.providers.map((each) => (each.provider === provider ? entry : each));
        }),
      );
    } catch (failure) {
      this.error = message(failure);
    }
  }

  /** Ask the library which model a new session on `worktree` starts with; the dialog says it is
   * reading until it answers. */
  async readStarting(worktree: string): Promise<void> {
    this.starting = { worktree, known: false, model: null };
    try {
      const model = await call(this.ipc.chatDefaultModel(worktree));
      if (this.starting?.worktree === worktree) this.starting = { worktree, known: true, model };
    } catch (failure) {
      if (this.starting?.worktree === worktree) this.starting = { worktree, known: true, model: null };
      this.error = message(failure);
    }
  }

  /** Start a session on `worktree`, on the library's default model, and show it. */
  async create(worktree: string): Promise<boolean> {
    // Without a model yet the dialog says why and does not offer Create.
    if (this.starting?.worktree !== worktree || this.starting.model === null) return false;
    try {
      const session = await call(this.ipc.chatCreate(worktree, null));
      await this.list();
      await this.open(session.id);
      return true;
    } catch (failure) {
      this.error = message(failure);
      return false;
    }
  }
}
