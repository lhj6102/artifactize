<script lang="ts">
  // Chat: Artifactize's own agent on a branch (J-CHAT…). Its Pane holds New chat and the
  // sessions, grouped by project, newest first; its main area holds the shown session's
  // conversation in a column of the reading measure and the composer under it, the model
  // selector at its bottom left (its menu opening upward) and Send, or Stop while a turn runs,
  // at its bottom right. Each verify the agent starts is a Run reference that opens the Run in
  // Verify. Sessions, turns and Run cards are the library's.
  import { Dialog } from "bits-ui";
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick, untrack } from "svelte";
  import { Coalesced } from "../ipc/coalesced";
  import type { Attachment } from "svelte/attachments";
  import { onSessionChanged, onStateChanged, type ChatRun, type ChatSession } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import { statusIcon } from "../shell/icons";
  import { tone } from "../shell/tone";
  import Panel from "../shell/Panel.svelte";
  import Reference from "../shell/Reference.svelte";
  import { composing } from "../shell/keys";
  import { providerName } from "../settings/provider-names";
  import { toolLine } from "./tool-line";
  import { useCommands } from "../shell/commands.svelte";
  import { keyOf } from "../monitor/monitor-state.svelte";

  const app = getAppState();
  const { browse, chat: c } = app;

  const branches = $derived(browse.view?.branches ?? []);
  /** Where a session works, as the library names it: its project and branch. */
  const placeOf = (session: ChatSession) => (session.branch ? `${session.project} / ${session.branch}` : session.project);
  /** The sessions grouped by project and branch, the groups and their sessions newest first. */
  const groups = $derived.by(() => {
    const byPlace: [string, ChatSession[]][] = [];
    for (const session of c.sessions) {
      const place = placeOf(session);
      const group = byPlace.find(([name]) => name === place);
      if (group) group[1].push(session);
      else byPlace.push([place, [session]]);
    }
    return byPlace;
  });
  const view = $derived(c.view);
  const session = $derived(view?.session ?? null);
  const running = $derived(view?.running ?? false);
  const model = $derived(session ? c.model(session) : null);
  const typed = $derived(session ? (c.typed[session.id] ?? "") : "");
  /** The model New chat's session starts with, once the library said it for the chosen branch. */
  const answer = $derived(c.starting !== null && c.starting.worktree === c.worktree ? c.starting : null);
  const starting = $derived(answer?.model ?? null);
  const reading = $derived(answer === null || !answer.known);

  /** How long ago `at` (an RFC 3339 time) was, as the Pane says it. */
  function age(at: string): string {
    const seconds = Math.max(0, (Date.now() - Date.parse(at)) / 1000);
    if (seconds < 60) return "now";
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
    return `${Math.floor(seconds / 86400)}d`;
  }

  /** A Run card's status word and its icon's status. */
  function runStatus(card: ChatRun): { word: string; status: "RUNNING" | "GREEN" | "RED" | "ERROR" | "YELLOW" | "WAITING_HUMAN" | "QUEUED" } {
    switch (card.state) {
      case "green":
        return { word: "GREEN", status: "GREEN" };
      case "red":
        return { word: "RED", status: "RED" };
      case "error":
      case "cancelled":
      case "incomplete":
        return { word: (card.state ?? "").toUpperCase(), status: "ERROR" };
      case "yellow":
        return { word: "YELLOW", status: "YELLOW" };
      case "waiting_human":
        return { word: "Waiting", status: "WAITING_HUMAN" };
      case "preparing":
      case "waiting":
        return { word: "Queued", status: "QUEUED" };
      default:
        return { word: "Running", status: "RUNNING" };
    }
  }

  function write(text: string) {
    if (session) c.typed[session.id] = text;
  }

  /** Enter sends, Shift+Enter breaks the line, Esc stops a running turn; an input method's
   * keys are its own. */
  function composerKey(event: KeyboardEvent) {
    if (composing(event) || event.ctrlKey || event.metaKey || event.altKey) return;
    // The composer's own keys: Enter is how it sends, as a field with no button would write.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void c.send();
    } else if (event.key === "Escape" && running) {
      event.preventDefault();
      void c.stop();
    }
  }

  async function openSession(id: string) {
    await c.open(id);
    await tick();
    c.panes.composer?.focus();
  }

  function pick(backend: string, slug: string) {
    if (session) c.chosen[session.id] = { backend, model: slug, effort: null };
    c.picking = false;
  }

  function newChat() {
    // The live branches as they are now: Verify reads them only while it is on screen.
    browse.refresh();
    // The branch selected in Verify comes first.
    const shown = app.monitor.shown;
    const selected = shown.kind === "none" ? undefined : branches.find((branch) => keyOf(branch.key) === keyOf(shown.branch));
    c.worktree = selected?.workspace ?? branches[0]?.workspace ?? null;
    c.creating = true;
    if (c.worktree) void c.readStarting(c.worktree);
  }

  // New chat from the palette too, whatever mode is shown: it shows Chat first (J-PALETTE).
  useCommands(() => [
    {
      id: "chat.new",
      group: "Chat",
      label: "New chat",
      run: () => {
        app.monitor.settings = false;
        app.mode = "chat";
        newChat();
      },
    },
  ]);

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (!c.worktree) return;
    if (await c.create(c.worktree)) {
      c.creating = false;
      await tick();
      c.panes.composer?.focus();
    }
  }

  /** The conversation follows its newest line, and brings a Run reference it was opened for into
   * view once that reference is drawn. */
  const follow: Attachment<HTMLElement> = (log) => {
    void view;
    const reveal = c.reveal;
    queueMicrotask(() => {
      const card = reveal ? log.querySelector<HTMLElement>(`[data-run="${CSS.escape(reveal)}"]`) : null;
      if (card) {
        card.scrollIntoView({ block: "center" });
        c.reveal = null;
      } else if (!reveal) log.scrollTop = log.scrollHeight;
    });
  };

  /** The shown session and the list read again as the library changes them, one read of each at
   * a time and only while Chat is on screen; what changed meanwhile is read when it is shown. */
  const rereading = new Coalesced(() => c.read(), () => app.mode === "chat");
  const listing = new Coalesced(() => c.list(), () => app.mode === "chat");
  const following: Attachment = () => {
    if (app.mode !== "chat") return;
    untrack(() => {
      rereading.showing();
      listing.showing();
    });
  };

  onMount(() => {
    void c.list();
    const unsession = onSessionChanged((id) => {
      if (id === c.selected) rereading.request();
      listing.request();
    });
    // A Run a session started moves on, or a checkout changed: the cards and sessions follow.
    const unstate = onStateChanged(() => {
      rereading.request();
      listing.request();
    });
    return () => {
      unsession();
      unstate();
    };
  });
</script>

{#snippet runCard(card: ChatRun)}
  {@const status = runStatus(card)}
  {@const attention = [...card.failed, ...card.waiting.map((name) => `${name} waits for a person`)].join(", ")}
  <div class="run" data-run={card.id}>
    <span class={tone(status.status)}><Icon name={statusIcon(status.status)} /></span>
    <span class="mono">{card.id}</span>
    <span class={tone(status.status)}>{status.word}</span>
    <span class="what">{attention}</span>
    <Reference onclick={() => app.showRun(card.id)}>Open in Verify</Reference>
  </div>
{/snippet}

<div class="mode" {@attach following}>
  <PaneGroup direction="horizontal" autoSaveId="chat-layout" class="columns">
    {#if app.paneShown}
      <Split id="chat-pane" order={1} defaultSize={20} minSize={12}>
        <section class="pane" aria-label="Chats" bind:this={c.panes.pane}>
          <div class="head"><button class="button new" onclick={newChat}><Icon name="add" />New chat</button></div>
          <div class="sessions">
            {#each groups as [project, members] (project)}
              <p class="group">{project}</p>
              <ul aria-label="Sessions of {project}">
                {#each members as item (item.id)}
                  <li>
                    <button class="session" class:selected={c.selected === item.id} aria-current={c.selected === item.id ? "true" : undefined} onclick={() => void openSession(item.id)}>
                      <span class="title">{item.title || "New chat"}</span><span class="age">{age(item.updatedAt)}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/each}
          </div>
        </section>
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="chat-main" order={2} defaultSize={app.paneShown ? 80 : 100} minSize={30}>
      <Panel title={session ? session.title || "New chat" : "Chat"} label="Chat's main area" focused={false} notice={c.error} bind:noticeOpen={c.noticeOpen} bind:element={c.panes.main}>
        {#snippet headline()}
          {#if session}<span class="branch mono">{session.branch || session.project}</span>{/if}
        {/snippet}
        {#if !session}
          <p class="empty">{c.listed && c.sessions.length === 0 ? "Start a session with New chat." : "Choose a session in the Pane."}</p>
        {:else}
          <div class="chat">
            <div class="convo" role="log" aria-label="Conversation" {@attach follow}>
              <div class="column">
                {#each view?.turns ?? [] as turn, at (at)}
                  <div class="message">{turn.user}</div>
                  <div class="turn">
                    {#each turn.blocks as block, part (part)}
                      {#if block.kind === "text"}
                        <p class="text">{block.text}</p>
                      {:else if block.kind === "tool"}
                        {@const busy = block.output === null && block.failed === null}
                        {@const line = toolLine(block.name, block.arguments, busy)}
                        <div class="call">
                          {#if busy}<span class="tone-running"><Icon name="running" /></span>{:else}<span class="quiet"><Icon name={line.icon} /></span>{/if}
                          <span>{line.verb}</span><span class="target mono">{line.target}</span>
                          {#if line.lines}<span>{line.lines}</span>{/if}
                          {#if block.failed}<span class="failed">{block.output}</span>{/if}
                        </div>
                      {:else}
                        {@render runCard(block.card)}
                      {/if}
                    {/each}
                    {#if turn.state.kind === "stopped" || turn.state.kind === "interrupted"}
                      <p class="end">Stopped</p>
                    {:else if turn.state.kind === "failed"}
                      <p class="end failed">{turn.state.message}</p>
                    {/if}
                  </div>
                {/each}
              </div>
            </div>
            <div class="composer">
              {#if session.unavailable}
                <!-- The session's branch cannot be worked on now: it says why, and sends nothing. -->
                <p class="unavailable" role="status">{session.unavailable}</p>
              {/if}
              <div class="box" class:off={session.unavailable !== null}>
                <textarea
                  class="bare-field"
                  rows="2"
                  aria-label="Message"
                  disabled={session.unavailable !== null}
                  placeholder="Ask the agent to work on this branch…"
                  bind:this={c.panes.composer}
                  value={typed}
                  oninput={(event) => write(event.currentTarget.value)}
                  onkeydown={composerKey}
                ></textarea>
                <div class="row">
                  <div class="picker">
                    <button class="model" aria-haspopup="menu" aria-expanded={c.picking} onclick={() => { c.picking = !c.picking; if (c.picking) void c.providersRead(); }}>
                      {model?.model ?? "Choose a model"}<Icon name="unfold" />
                    </button>
                    {#if c.picking}
                      <!-- The model menu, opening upward from the selector. -->
                      <div class="menu" role="menu" aria-label="Models">
                        {#each c.providers as entry (entry.provider)}
                          <p class="label">{providerName(entry.provider)}</p>
                          {#if entry.error}
                            <p class="note">{entry.error}</p>
                          {:else if entry.models === null}
                            <p class="quiet-note">…</p>
                          {:else}
                            {#each entry.models as item (item.slug)}
                              {@const on = model?.backend === entry.provider && model.model === item.slug}
                              <button class="item" class:on role="menuitemradio" aria-checked={on} onclick={() => pick(entry.provider, item.slug)}>{item.displayName}</button>
                            {/each}
                          {/if}
                        {/each}
                        <hr />
                        <button class="item" role="menuitem" onclick={() => { c.picking = false; app.monitor.settings = true; }}><Icon name="add" />Connect provider</button>
                      </div>
                    {/if}
                  </div>
                  <span class="spacer"></span>
                  {#if running}
                    <button class="button icon-button" aria-label="Stop (Esc)" aria-keyshortcuts="Escape" onclick={() => void c.stop()}><Icon name="stop" /></button>
                  {:else}
                    <button class="button icon-button primary" aria-label="Send (Enter)" aria-keyshortcuts="Enter" disabled={typed.trim() === "" || session.unavailable !== null} onclick={() => void c.send()}><Icon name="send" /></button>
                  {/if}
                </div>
              </div>
            </div>
          </div>
        {/if}
      </Panel>
    </Split>
  </PaneGroup>
</div>

<!-- New chat: the branch the session works on. -->
<Dialog.Root bind:open={c.creating}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog new-chat">
      <Dialog.Title class="dialog-title">New chat</Dialog.Title>
      <form class="dialog-layout" onsubmit={create}>
        <div class="dialog-body">
          <label class="field">
            <span class="field-name">Branch</span>
            <select bind:value={c.worktree} onchange={() => c.worktree && void c.readStarting(c.worktree)}>
              {#each branches as branch (branch.workspace)}
                <option value={branch.workspace}>{branch.name ? `${branch.project} / ${branch.name}` : branch.project}</option>
              {/each}
            </select>
          </label>
          <div class="field">
            <span class="field-name" id="new-chat-model">Model</span>
            <p class="starting" class:quiet={!starting} role="status" aria-labelledby="new-chat-model">
              {#if starting}
                {providerName(starting.backend)} · {starting.model}
              {:else if reading}
                Reading the providers' models…
              {:else}
                No provider lists a model. Connect one in Settings.
              {/if}
            </p>
          </div>
        </div>
        <div class="dialog-actions">
          <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
          <button class="button primary" type="submit" aria-keyshortcuts="Enter" disabled={!c.worktree || !starting}>Create</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .mode {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .mono {
    font-family: var(--mono);
  }
  /* The sidebar: New chat across its top, then the sessions under their project (D-CHAT-1). */
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
  }
  .head {
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--line-group);
  }
  .new {
    width: 100%;
    justify-content: flex-start;
    gap: 6px;
  }
  .sessions {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding-bottom: 4px;
  }
  .sessions ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .group {
    margin: 0;
    padding: 10px var(--pad) 4px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .session {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row-pane);
    margin: 0;
    padding: 0 var(--pad);
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .session:not(.selected):hover {
    background: var(--surface-group);
  }
  .session.selected {
    background: var(--selection);
    color: var(--ink);
  }
  .session .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .age {
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }
  .branch {
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  /* The conversation on the working surface, a column of the reading measure (D-CHAT-2). */
  .chat {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    height: 100%;
    min-height: 0;
    border-top: 1px solid var(--line-panel);
    background: var(--surface-app);
  }
  .convo {
    min-height: 0;
    overflow: auto;
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: var(--measure);
    margin: 0 auto;
    padding: 16px var(--pad) 12px;
  }
  .message {
    align-self: flex-end;
    max-width: 80%;
    padding: 6px 10px;
    border-radius: var(--radius-chip);
    background: var(--surface-group);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .turn {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .text {
    margin: 2px 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* What the agent did: one quiet line per tool, no box (D-CHAT-4). */
  .call {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: var(--row-dense);
    color: var(--ink-2);
    font-size: var(--text-small);
    white-space: nowrap;
  }
  .quiet {
    display: inline-flex;
    color: var(--ink-3);
  }
  .tone-running {
    display: inline-flex;
  }
  .target,
  .failed {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .failed {
    color: var(--red);
  }
  .end {
    margin: 2px 0;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .end.failed {
    color: var(--red);
    white-space: pre-wrap;
  }
  /* A Run reference (D-CHAT-5). */
  .run {
    display: grid;
    grid-template-columns: var(--icon) max-content max-content minmax(0, 1fr) max-content;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 0 10px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    font-size: var(--text-small);
  }
  .run > span:first-child {
    display: inline-flex;
  }
  .what {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  /* The composer: one field, the model at its bottom left and Send at its bottom right
     (D-CHAT-6). */
  .composer {
    width: 100%;
    max-width: var(--measure);
    margin: 0 auto 12px;
    padding: 0 var(--pad);
    box-sizing: border-box;
  }
  .box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-chip);
    background: var(--surface-input);
  }
  /* A session that cannot send reads as off; why stands above its composer. */
  .box.off {
    opacity: 0.6;
  }
  .unavailable {
    margin: 0 0 6px;
    color: var(--ink-2);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  .box:has(textarea:focus) {
    outline: 1px solid var(--current-mark);
    outline-offset: -2px;
  }
  .box:has(textarea:focus-visible) {
    box-shadow: 0 0 0 2px var(--surface-app), 0 0 0 4px var(--focus);
  }
  textarea {
    min-height: 36px;
    max-height: 12em;
    padding: 0;
    border: 0;
    outline: none;
    resize: none;
    field-sizing: content;
    background: none;
    color: var(--ink);
    font: inherit;
  }
  textarea::placeholder {
    color: var(--ink-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  .spacer {
    flex: 1;
  }
  .picker {
    position: relative;
  }
  .model {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--control);
    margin: 0;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .model:hover {
    background: var(--surface-group);
  }
  /* The model menu, raised, opening upward (D-CHAT-7). */
  .menu {
    position: absolute;
    left: 0;
    bottom: calc(100% + 4px);
    z-index: 30;
    width: 260px;
    max-height: 60vh;
    overflow: auto;
    padding: 4px;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .menu .label {
    margin: 0;
    padding: 6px 8px 2px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .menu .note {
    margin: 0;
    padding: 0 8px 4px;
    color: var(--red);
    font-size: var(--text-small);
  }
  .menu .quiet-note {
    margin: 0;
    padding: 0 8px 4px;
    color: var(--ink-3);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: var(--row);
    margin: 0;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:not(.on):hover {
    background: var(--surface-group);
  }
  .item.on {
    background: var(--selection);
    color: var(--ink);
  }
  hr {
    margin: 4px 0;
    border: 0;
    border-top: 1px solid var(--line-group);
  }
  .icon-button {
    display: inline-grid;
    place-items: center;
    width: var(--control);
    padding: 0;
  }
  :global(.dialog.new-chat) {
    width: min(420px, 92vw);
  }
  .field {
    display: grid;
    gap: 4px;
  }
  .field-name {
    font-size: var(--text-small);
    color: var(--label);
  }
  .starting {
    margin: 0;
    min-height: var(--control);
    display: flex;
    align-items: center;
    font: var(--text-small) var(--mono);
  }
  .starting.quiet {
    color: var(--ink-2);
    font-family: inherit;
  }
  select {
    height: var(--control);
    padding: 0 6px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: var(--text-small) var(--mono);
  }
</style>
