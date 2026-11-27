// Chat mode's own facts: the sessions the library lists, the session shown with its transcript,
// the target (ACP agent + configOptions, or direct HTTP model + effort) chosen for its next turn,
// Workspace Trust prompts, interactive permission responses, the Run reference to bring into view,
// the picker's agents/providers, and what is typed in each composer. One owner, kept in AppState.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { GraphWorkspace } from "../monitor/graph-workspace.svelte";
import {
  call,
  message,
  type BranchKey,
  type ChatBranch,
  type ChatSession,
  type ChatSessionView,
  type ChatTarget,
  type ChatTrustRequirement,
  type ChatView,
  type Commands,
} from "../ipc/client";
import { keyOf } from "../monitor/monitor-state.svelte";
import { providerEntries, readAgent, readProvider, type MenuAgent, type MenuProvider } from "./model-providers";

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
  readonly graph = $derived(this.selected === null ? null : (this.#graphs.get(this.selected) ?? null));
  readonly scroll: Record<string, { top: number; following: boolean }> = $state({});
  /** A Run reference to bring into view once the shown session is read (J-RUN-ORIGIN). */
  reveal = $state<string | null>(null);
  /** What is typed in each session's composer, by session: kept while another is shown. */
  readonly #composers: Record<string, Composer> = Object.create(null);
  composer(id: string): Composer | undefined {
    return this.#composers[id];
  }
  /** Work tied to a session, even if the person switches away while the command runs. */
  readonly sending = new SvelteMap<string, boolean>();
  readonly stopping = new SvelteMap<string, boolean>();
  readonly choosing = new SvelteMap<string, boolean>();
  readonly rejected = new SvelteMap<string, { text: string; reason: string }>();
  readonly respondingPermission = new SvelteSet<string>();
  readonly #marking = new SvelteSet<string>();
  /** ACP Chat agents and direct HTTP providers for the picker menu. */
  agents = $state.raw<MenuAgent[]>([]);
  providers = $state.raw<MenuProvider[]>([]);
  providerError = $state<string | null>(null);
  modelQuery = $state("");
  /** Pending Workspace Trust modal when `chat_send` returns `trustRequired`. */
  trustPrompt = $state.raw<{
    sessionId: string;
    worktree: string;
    requirement: ChatTrustRequirement;
    busy: boolean;
    error: string | null;
  } | null>(null);
  /** The target a new session on a worktree starts with, as the library says. */
  starting = $state.raw<{ worktree: string; branch: BranchKey | null; known: boolean; target: ChatTarget | null } | null>(null);
  /** What went wrong, for the main area's notice. */
  error = $state<string | null>(null);
  noticeOpen = $state(false);
  /** The New chat dialog is open, and the worktree it would start the session in. */
  creating = $state(false);
  branches = $state.raw<ChatBranch[] | null>(null);
  branchError = $state<string | null>(null);
  worktree = $state<string | null>(null);
  /** Move chat: the session it moves, the checkout chosen, the live checkouts once read, what
   * the library refused, and whether the move is under way; null while closed. */
  moving = $state.raw<{ id: string; worktree: string; branches: ChatBranch[] | null; error: string | null; busy: boolean } | null>(null);
  /** The target/model menu is open. */
  picking = $state(false);
  /** The Pane's top row is the search field (J-CHAT). */
  searching = $state(false);
  /** The projects folded in the Pane, by name. */
  readonly folded = new SvelteSet<string>();
  /** Each session's reply card in its Graph view (J-CHAT-GRAPH). */
  readonly replies = new SvelteMap<string, { at: number; folded: boolean }>();
  /** Find in the conversation (J-CHAT-FIND). */
  find = $state<{ text: string; at: number } | null>(null);
  /** How many places in the shown conversation hold the find field's text, as drawn. */
  found = $state(0);
  /** How far the floating chat reaches over the graph, its 12px gap included (D-CUR-10). */
  band = $state(0);
  /** The parts focus moves into. */
  readonly panes: {
    pane?: HTMLElement;
    search?: HTMLInputElement;
    searchTool?: HTMLButtonElement;
    main?: HTMLElement;
    composer?: HTMLTextAreaElement;
  } = $state({});

  /** Counts reads of the shown session: only the newest answer is shown. */
  #reads = 0;
  #opening = 0;
  #views = 0;
  #lists = 0;
  #providerReads = 0;
  #defaults = 0;

  constructor(
    readonly ipc: Commands,
    readonly shown: () => boolean,
  ) {}

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
      this.find = null;
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
      if (kind === "conversation") this.replies.delete(id);
      else this.find = null;
      if (changing !== this.#views || this.view?.session.id !== id) return;
      this.view = { ...this.view, session };
      this.error = null;
      await this.read();
    } catch (failure) {
      if (changing === this.#views) this.error = message(failure);
    }
  }

  /** Open New chat on a branch from the library's full list, not Verify's search result. */
  async newChat(branch: BranchKey | null, exact: boolean, project: string | null = null): Promise<void> {
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
      const selected =
        (branch ? branches.find((item) => keyOf(item.key) === keyOf(branch)) : undefined) ??
        (project !== null ? branches.find((item) => item.project === project) : undefined) ??
        (exact ? undefined : branches[0]);
      if (selected) {
        this.worktree = selected.workspace;
        void this.readStarting(selected.workspace, selected.key);
      }
    } catch (error) {
      if (this.creating && opening === this.#opening) this.branchError = message(error);
    }
  }

  /** Open Move chat for the shown session: the live checkouts, its own first (J-CHAT-MOVE). */
  async openMove(): Promise<void> {
    const view = this.view;
    if (!view || view.activity !== "idle") return;
    const id = view.session.id;
    this.moving = { id, worktree: view.session.worktree, branches: null, error: null, busy: false };
    try {
      const branches = await call(this.ipc.chatBranches());
      if (this.moving?.id !== id) return;
      const own = branches.filter((item) => item.workspace === view.session.worktree);
      this.moving = {
        ...this.moving,
        branches: [...own, ...branches.filter((item) => item.workspace !== view.session.worktree)],
      };
    } catch (failure) {
      if (this.moving?.id === id) this.moving = { ...this.moving, branches: [], error: message(failure) };
    }
  }

  /** Move the session to the checkout chosen in Move chat; a refusal stays in the dialog. */
  async move(): Promise<boolean> {
    const moving = this.moving;
    if (!moving || moving.busy) return false;
    this.moving = { ...moving, busy: true, error: null };
    try {
      const session = await call(this.ipc.chatSetWorktree(moving.id, moving.worktree));
      if (this.moving?.id !== moving.id) return false;
      this.moving = null;
      if (this.view?.session.id === session.id) this.view = { ...this.view, session };
      void this.read();
      void this.list();
      return true;
    } catch (failure) {
      if (this.moving?.id === moving.id) this.moving = { ...this.moving, busy: false, error: message(failure) };
      return false;
    }
  }

  /** Read the shown session again: its turns stream, permissions update, and Run cards change. */
  async read(acknowledge = true): Promise<void> {
    const id = this.selected;
    if (id === null) return;
    const mine = ++this.#reads;
    try {
      const view = await call(this.ipc.chatOpen(id));
      if (mine !== this.#reads || this.selected !== id) return;
      this.view = view;
      if (view.activity === "idle") this.stopping.delete(id);
      if (acknowledge) this.acknowledge();
    } catch (failure) {
      if (mine === this.#reads && this.selected === id) this.error = message(failure);
    }
  }

  acknowledge(): void {
    if (this.view?.session.unseen && this.view.activity === "idle" && this.shown()) void this.#markSeen(this.view.session.id);
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

  /** A pending message is neither in the transcript nor eligible to send twice.
   * If an untrusted ACP workspace triggers `trustRequired`, the typed prompt stays intact and
   * the Workspace Trust dialog opens before anything is written to JSONL. */
  async send(): Promise<void> {
    const view = this.view;
    if (
      !view ||
      view.activity !== "idle" ||
      view.session.unavailable !== null ||
      view.session.target === null ||
      view.session.target.engine === "legacyClaudeAgent"
    ) {
      return;
    }
    const id = view.session.id;
    const composer = this.composer(id);
    if (!composer) return;
    const text = composer.shown;
    if (text.trim() === "" || this.sending.has(id) || this.choosing.has(id) || this.stopping.has(id)) return;
    this.sending.set(id, true);
    this.rejected.delete(id);
    const finish = composer.begin();
    try {
      const outcome = await call(this.ipc.chatSend(id, text, null));
      if (outcome.kind === "trustRequired") {
        finish(false);
        this.trustPrompt = {
          sessionId: id,
          worktree: view.session.worktree,
          requirement: outcome.requirement,
          busy: false,
          error: null,
        };
        return;
      }
      finish(true);
      if (view.session.view === "graph") this.replies.set(id, { at: view.turns.length, folded: false });
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

  /** Record explicit Workspace Trust and retry the waiting send seamlessly. */
  async confirmTrust(): Promise<void> {
    const prompt = this.trustPrompt;
    if (!prompt || prompt.busy) return;
    this.trustPrompt = { ...prompt, busy: true, error: null };
    try {
      await call(this.ipc.chatTrustGrant(prompt.worktree, prompt.requirement.agentId));
      this.trustPrompt = null;
      if (this.selected === prompt.sessionId) {
        await this.send();
      }
    } catch (failure) {
      if (this.trustPrompt?.sessionId === prompt.sessionId) {
        this.trustPrompt = { ...prompt, busy: false, error: message(failure) };
      }
    }
  }

  /** Submit a user's choice for a pending ACP permission card (`PENDING -> SELECTED`). */
  async respondPermission(sessionId: string, permissionId: string, optionId: string): Promise<void> {
    if (this.respondingPermission.has(permissionId)) return;
    this.respondingPermission.add(permissionId);
    try {
      await call(this.ipc.chatPermissionRespond(sessionId, permissionId, optionId));
      if (this.selected === sessionId) await this.read();
      void this.list();
    } catch (failure) {
      if (this.selected === sessionId) this.error = message(failure);
    } finally {
      this.respondingPermission.delete(permissionId);
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
    if (!view || view.activity === "idle" || this.stopping.has(view.session.id)) return;
    const id = view.session.id;
    this.stopping.set(id, true);
    this.error = null;
    try {
      const requested = await call(this.ipc.chatStop(id));
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

  /** Save the selected ChatTarget (ACP agent + configOptions, or direct HTTP model + effort). */
  async setTarget(id: string, selection: ChatTarget): Promise<void> {
    this.picking = false;
    if (this.choosing.has(id)) return;
    this.choosing.set(id, true);
    try {
      const saved = await call(this.ipc.chatSetTarget(id, selection));
      if (this.view?.session.id === id) {
        ++this.#reads;
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

  /** Read ACP Chat agents and direct HTTP providers for the picker menu. */
  async openPicker(): Promise<void> {
    this.picking = true;
    this.modelQuery = "";
    const mine = ++this.#providerReads;
    this.providerError = null;
    try {
      const [listedAgents, listedProviders] = await Promise.all([
        call(this.ipc.chatAgentsList()),
        call(this.ipc.providersList()),
      ]);
      if (mine !== this.#providerReads) return;
      this.agents = listedAgents.map((entry) => ({ entry, availability: { kind: "loading" } }));
      const entries = providerEntries(listedProviders);
      this.providers = entries.map(({ provider, directBackend }) => ({
        provider,
        directBackend,
        listing: { kind: "loading" },
      }));
      await Promise.all([
        ...listedAgents.map(async (entry) => {
          const availability = await readAgent(this.ipc, entry.id);
          if (mine === this.#providerReads) {
            this.agents = this.agents.map((item) => (item.entry.id === entry.id ? { entry, availability } : item));
          }
        }),
        ...entries.map(async ({ provider, directBackend }) => {
          const listing = await readProvider(this.ipc, provider);
          if (mine === this.#providerReads) {
            this.providers = this.providers.map((item) =>
              item.provider === provider ? { provider, directBackend, listing } : item,
            );
          }
        }),
      ]);
    } catch (failure) {
      if (mine === this.#providerReads) this.providerError = message(failure);
    }
  }

  /** Ask the library which target a new session on `worktree` starts with. */
  async readStarting(worktree: string, branch: BranchKey | null): Promise<void> {
    const mine = ++this.#defaults;
    this.starting = { worktree, branch, known: false, target: null };
    try {
      const target = await call(this.ipc.chatDefaultTarget(worktree, branch));
      if (mine === this.#defaults) this.starting = { worktree, branch, known: true, target };
    } catch (failure) {
      if (mine !== this.#defaults) return;
      this.starting = { worktree, branch, known: true, target: null };
      this.error = message(failure);
    }
  }

  /** Start a session with the displayed default, or without a target when none was ready. */
  async create(worktree: string): Promise<boolean> {
    const starting = this.starting;
    if (starting?.worktree !== worktree || !starting.known) return false;
    try {
      const session = await call(this.ipc.chatCreate(worktree, starting.branch, starting.target));
      await this.list();
      await this.open(session.id);
      return true;
    } catch (failure) {
      this.error = message(failure);
      return false;
    }
  }
}
