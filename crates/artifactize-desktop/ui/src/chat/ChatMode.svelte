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
  import { mainSplitMinimum, PANE_MIN } from "../monitor/detail-width";
  import Panel from "../shell/Panel.svelte";
  import Reference from "../shell/Reference.svelte";
  import { composing } from "../shell/keys";
  import { providerName } from "../settings/provider-names";
  import { activity } from "./tool-line";
  import ChatActivity from "./ChatActivity.svelte";
  import ChatMarkdown from "./ChatMarkdown.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import CurrentView from "./CurrentView.svelte";
  import ScanState from "./ScanState.svelte";

  const app = getAppState();
  const { current, chat: c } = app;
  const definitions = $derived(app.definitions);

  const branches = $derived(c.branches ?? []);
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
  const turns = $derived((view?.turns ?? []).map((turn) => ({ ...turn, activity: activity(turn.blocks) })));
  const graph = $derived(session?.view === "graph");
  const mainMin = $derived(graph ? mainSplitMinimum(app.windowWidth) : 30);
  const live = $derived(graph && session?.unavailable === null);
  const mainNotice = $derived((live ? definitions.notice ?? c.graph?.graph.error : null) ?? c.error);
  const running = $derived(view?.running ?? false);
  const stopping = $derived(session !== null && c.stopping.has(session.id));
  const submitting = $derived(session !== null && (c.sending.has(session.id) || c.choosing.has(session.id)));
  const rejected = $derived(session === null ? undefined : c.rejected.get(session.id));
  const away = $derived(session !== null && c.scroll[session.id]?.following === false);
  const typed = $derived(session ? c.composer(session.id)?.shown ?? "" : "");
  /** The model New chat's session starts with, once the library said it for the chosen branch. */
  const answer = $derived(c.starting !== null && c.starting.worktree === c.worktree ? c.starting : null);
  const starting = $derived(answer?.model ?? null);
  const reading = $derived(answer === null || !answer.known);

  /** How long ago `at` (an RFC 3339 time) was, as the Pane says it. */
  function age(at: string, now: number): string {
    const seconds = Math.max(0, (now - Date.parse(at)) / 1000);
    if (seconds < 60) return "now";
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
    return `${Math.floor(seconds / 86400)}d`;
  }

  const ages = $derived(new Map(c.sessions.map((item) => [item.id, age(item.updatedAt, app.now)])));

  function sessionState(item: ChatSession) {
    if (item.outcome === "running" || item.running) return "running";
    return item.unseen ? item.outcome === "completed" ? "finished" : item.outcome : null;
  }

  /** A Run card's status word and its icon's status. */
  function runStatus(card: ChatRun): { word: string; status: "RUNNING" | "GREEN" | "RED" | "ERROR" | "YELLOW" | "WAITING_HUMAN" | "QUEUED" | "INCOMPLETE" } {
    switch (card.state) {
      case "green":
        return { word: "GREEN", status: "GREEN" };
      case "red":
        return { word: "RED", status: "RED" };
      case "error":
      case "cancelled":
        return { word: (card.state ?? "").toUpperCase(), status: "ERROR" };
      case "incomplete":
        return { word: "INCOMPLETE", status: "INCOMPLETE" };
      case "yellow":
        return { word: "YELLOW", status: "YELLOW" };
      case "waiting_human":
        return { word: "Waiting", status: "WAITING_HUMAN" };
      case "preparing":
      case "waiting":
        return { word: "Queued", status: "QUEUED" };
      case "running":
        return { word: "Running", status: "RUNNING" };
      case "cancelling":
        return { word: "Cancelling", status: "RUNNING" };
      default:
        return { word: "Status unavailable", status: "INCOMPLETE" };
    }
  }

  function write(text: string) {
    if (session) c.composer(session.id)?.type(text);
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
      if (!c.picking) void c.stop();
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (!c.worktree) return;
    if (await c.create(c.worktree)) {
      c.creating = false;
      await tick();
      c.panes.composer?.focus();
    }
  }

  /** Read the same place again after a view or session switch; only follow at the end. */
  function remember(log: HTMLElement) {
    const id = session?.id;
    if (!id || app.mode !== "chat") return;
    const top = log.scrollTop;
    // A programmatic reveal may already be at the bottom; only an actual scroll or the button
    // should end that reading position.
    if (c.scroll[id]?.top === top) return;
    c.scroll[id] = { top, following: log.scrollHeight - log.clientHeight - top <= 2 };
  }

  function scrollToEnd(log: HTMLElement, id: string) {
    log.scrollTop = log.scrollHeight;
    c.scroll[id] = { top: log.scrollTop, following: true };
  }

  const follow: Attachment<HTMLElement> = (log) => {
    const id = session?.id;
    const reveal = c.reveal;
    const turns = view?.turns; // Recheck when a library snapshot brings a new line or Run card.
    if (!id || !turns || app.mode !== "chat") return;
    let attached = true;
    function place() {
      if (!attached || !id || app.mode !== "chat") return;
      const card = reveal && [...log.querySelectorAll<HTMLElement>("[data-run]")].find((item) => item.dataset.run === reveal);
      if (card && c.reveal === reveal) {
        card.scrollIntoView({ block: "center" });
        c.scroll[id] = { top: log.scrollTop, following: false };
        c.reveal = null;
      } else if (!c.reveal) {
        const kept = c.scroll[id];
        log.scrollTop = !kept || kept.following ? log.scrollHeight : kept.top;
      }
    }
    queueMicrotask(place);
    const dimensions = new ResizeObserver(place);
    dimensions.observe(log);
    if (log.firstElementChild) dimensions.observe(log.firstElementChild);
    return () => { attached = false; dimensions.disconnect(); };
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
      c.acknowledge();
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
  {@const attention = [...card.failed, ...card.waiting].join(", ")}
  <div class="run" data-run={card.id}>
    <span class={tone(status.status)}><Icon name={statusIcon(status.status)} /></span>
    <span class="mono">{card.id}</span>
    <b class={tone(status.status)}>{status.word}</b>
    <span class="what">{attention}</span>
    <Reference onclick={() => app.showRun(card.id)}>Open in Verify</Reference>
  </div>
{/snippet}


<div class="mode" {@attach following}>
  <PaneGroup direction="horizontal" autoSaveId="chat-layout" class="columns">
    {#if app.paneShown}
      <Split id="chat-pane" order={1} defaultSize={20} minSize={PANE_MIN}>
        <section class="pane" aria-label="Chats" bind:this={c.panes.pane}>
          <div class="head"><button class="button new" onclick={() => app.newChat()}><Icon name="add" />New chat</button></div>
          <div class="search"><input type="search" value={c.query} bind:this={c.panes.search} aria-label="Search chats" placeholder="Search chats" oninput={(event) => c.search(event.currentTarget.value)} onkeydown={(event) => {
            if (event.key === "Escape" && !composing(event)) {
              event.preventDefault();
              c.search("");
            }
          }} /></div>
          <div class="sessions">
            {#if c.listed && c.query.trim() && groups.length === 0}<p class="empty">No chat matches.</p>{/if}
            {#each groups as [project, members] (project)}
              <p class="group">{project}</p>
              <ul aria-label="Sessions of {project}">
                {#each members as item (item.id)}
                  {@const state = sessionState(item)}
                  <li>
                    <button class="session" class:selected={c.selected === item.id} aria-current={c.selected === item.id ? "true" : undefined} onclick={() => void c.open(item.id)}>
                      <span class="title">{item.title || "New chat"}</span> <span class="age" class:running={state === "running"} class:failed={state === "failed"} class:finished={state === "finished"}>{state ?? ages.get(item.id)}</span>
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
    <Split id="chat-main" order={2} defaultSize={app.paneShown ? 80 : 100} minSize={mainMin}>
      <Panel title={session ? session.title || "New chat" : "Chat"} label="Chat's main area" focused={false} notice={mainNotice} bind:noticeOpen={c.noticeOpen} bind:element={c.panes.main}>
        {#snippet headline()}
          {#if session}<span class="branch mono">{session.branch || session.project}</span>{/if}
        {/snippet}
        {#snippet afterHeadline()}
          {#if session}
            <div class="views" role="group" aria-label="Session view">
              <button type="button" aria-pressed={!graph} onclick={() => void c.setView("conversation")}>Conversation</button>
              <button type="button" aria-pressed={graph} onclick={() => void c.setView("graph")}>Graph</button>
            </div>
            {#if live}<ScanState branch={session.branchKey} />{/if}
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if session && live}
            <button class="button small" aria-label="Refresh (Ctrl+R)" aria-keyshortcuts="Control+R F5" onclick={() => void current.scan(session.branchKey, true)}>Refresh</button>
          {/if}
        {/snippet}
        {#if !session}
          <p class="empty">{c.listed && !c.query.trim() && c.sessions.length === 0 ? "Start a session with New chat." : "Choose a session in the Pane."}</p>
        {:else}
          <div class="chat" class:live>
            {#if graph}
              {#if session.unavailable !== null}
                <p class="empty" role="status">{session.unavailable}</p>
              {:else if c.graph}
                {#key session.id}
                  <CurrentView branch={session.branchKey} worktree={session.worktree} workspace={c.graph} />
                {/key}
              {/if}
            {:else}
              {#key session.id}
                <div class="reading">
                  <div class="convo" role="log" aria-label="Conversation" {@attach follow} onscroll={(event) => remember(event.currentTarget)}>
                    <div class="column">
                      {#each turns as turn, at (at)}
                        <div class="message">{turn.user}</div>
                        <div class="turn">
                          {#each turn.activity as part (part.at)}
                            {#if part.kind === "group"}
                              <ChatActivity tools={part.tools} grouped running={turn.state.kind === "running"} />
                            {:else if part.block.kind === "text"}
                              <ChatMarkdown text={part.block.text} />
                            {:else if part.block.kind === "tool"}
                              <ChatActivity tools={[part.block]} running={turn.state.kind === "running"} />
                            {:else}
                              {@render runCard(part.block.card)}
                            {/if}
                          {/each}
                          {#if turn.state.kind === "stopped"}
                            <p class="end">Stopped</p>
                          {:else if turn.state.kind === "interrupted"}
                            <p class="end">Interrupted · Turn ended before the agent finished.</p>
                          {:else if turn.state.kind === "failed"}
                            <p class="end failed">{turn.state.message}</p>
                          {/if}
                        </div>
                      {/each}
                    </div>
                  </div>
                  {#if away}
                    <button class="button scroll-end" onclick={(event) => {
                      const log = event.currentTarget.previousElementSibling;
                      if (log instanceof HTMLElement) scrollToEnd(log, session.id);
                    }}><Icon name="unfold" />Scroll to end</button>
                  {/if}
                </div>
              {/key}
            {/if}
            <div class="composer">
              {#if !graph && session.unavailable}
                <!-- The session's branch cannot be worked on now: it says why, and sends nothing. -->
                <p class="unavailable" role="status">{session.unavailable}</p>
              {/if}
              {#if rejected}
                <div class="rejected">
                  <span class="failed" role="status">{rejected.reason}</span>
                  <button class="button small" onclick={() => void c.copy(rejected.text)}>Copy</button>
                  <button class="button small" disabled={typed !== ""} onclick={() => { c.restore(session.id); c.panes.composer?.focus({ preventScroll: true }); }}>Restore</button>
                </div>
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
                  <ModelPicker {session} />
                  <span class="spacer"></span>
                  {#if stopping}
                    <span class="stopping" role="status"><span class="tone-running"><Icon name="running" /></span>Stopping…</span>
                  {:else if running}
                    <button class="button icon-button" disabled={session.unavailable !== null} aria-label="Stop (Esc)" aria-keyshortcuts="Escape" onclick={() => void c.stop()}><Icon name="stop" /></button>
                  {:else}
                    <button class="button icon-button primary" aria-label="Send (Enter)" aria-keyshortcuts="Enter" disabled={typed.trim() === "" || session.unavailable !== null || submitting} onclick={() => void c.send()}><Icon name="send" /></button>
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
            <select bind:value={c.worktree} disabled={!branches.length} onchange={() => c.worktree && void c.readStarting(c.worktree, branches.find((branch) => branch.workspace === c.worktree)?.key ?? null)}>
              {#if c.worktree === null}<option value={null} disabled>Choose a branch</option>{/if}
              {#each branches as branch (branch.workspace)}
                <option value={branch.workspace}>{branch.name ? `${branch.project} / ${branch.name}` : branch.project}</option>
              {/each}
            </select>
          </label>
          <div class="field">
            <span class="field-name" id="new-chat-model">Model</span>
            <p class="starting" class:quiet={!starting} role="status" aria-labelledby="new-chat-model">
              {#if c.branchError}
                {c.branchError}
              {:else if c.branches === null}
                Reading branches…
              {:else if branches.length === 0}
                No checked-out branch.
              {:else if c.worktree === null}
                Choose a branch.
              {:else if answer?.error}
                {answer.error}
              {:else if starting}
                {providerName(starting.backend)} · {starting.model}{starting.effort ? ` · ${starting.effort}` : ""}
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
  .search { padding: 8px var(--pad) 4px; }
  .search input {
    box-sizing: border-box;
    width: 100%;
    height: var(--control);
    padding: 0 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: inherit;
  }
  .search input::placeholder { color: var(--ink-2); }
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
  .session:focus-visible { outline-offset: -2px; }
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
  .age.running { color: var(--running); }
  .age.failed { color: var(--red); }
  .age.finished { color: var(--ink); }
  .branch {
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .views {
    display: inline-flex;
    flex: none;
    height: var(--row-dense);
    box-sizing: border-box;
    margin-left: 4px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
  }
  .views button {
    border: 0;
    padding: 0 10px;
    background: none;
    color: var(--ink-off);
    font: inherit;
    cursor: pointer;
  }
  .views button:first-child {
    border-radius: calc(var(--radius-control) - 1px) 0 0 calc(var(--radius-control) - 1px);
  }
  .views button + button {
    border-left: 1px solid var(--line-input);
    border-radius: 0 calc(var(--radius-control) - 1px) calc(var(--radius-control) - 1px) 0;
  }
  .views button[aria-pressed="true"] {
    color: var(--ink-bright);
    box-shadow: inset 0 -2px 0 var(--current-mark);
  }
  .views button:hover { background: var(--surface-group); }
  .views button:focus-visible {
    outline-offset: -2px;
  }
  .views + :global(.scan) {
    margin-left: 12px;
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
  .chat.live { row-gap: 8px; }
  .reading { position: relative; display: grid; grid-template-rows: minmax(0, 1fr); min-height: 0; }
  .convo { min-height: 0; overflow: auto; overflow-anchor: none; }
  .scroll-end {
    position: absolute;
    justify-self: center;
    bottom: 8px;
    font-size: var(--text-small);
    gap: 4px;
    background: var(--surface-raised);
    border-color: var(--line-panel);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: var(--measure);
    min-height: 100%;
    box-sizing: border-box;
    justify-content: end;
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
  .failed { color: var(--red); }
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
  .run b { font-weight: inherit; }
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
    color: var(--ink-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  .spacer {
    flex: 1;
  }
  .rejected, .stopping { display: flex; align-items: center; gap: 8px; font-size: var(--text-small); }
  .rejected { margin: 0 0 6px; }
  .rejected .failed { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .stopping { color: var(--ink-2); min-height: var(--control); }
  .stopping .tone-running { display: inline-flex; }
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
