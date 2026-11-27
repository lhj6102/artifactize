<script lang="ts">
  // Chat: Artifactize's own agent on a branch (J-CHAT…). Its Pane's top row is New chat and
  // Search, then the projects, each a folder row over its sessions, newest first. Its main area
  // is the stage (J-STAGE): the view tabs and the shown view's tools in the stage header, then
  // the conversation in a column of the reading measure with the composer under it, or the
  // branch's Current with the composer and the reply card floating over its bottom
  // (J-CHAT-GRAPH). Each verify the agent starts is a Run reference that opens the Run in
  // Verify. Sessions, turns and Run cards are the library's.
  import { Dialog } from "bits-ui";
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick, untrack } from "svelte";
  import { Coalesced } from "../ipc/coalesced";
  import type { Attachment } from "svelte/attachments";
  import { onSessionChanged, onStateChanged, type ChatRun, type ChatSession } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { keyOf } from "../monitor/monitor-state.svelte";
  import Icon from "../shell/Icon.svelte";
  import { statusIcon } from "../shell/icons";
  import { tone } from "../shell/tone";
  import { mainSplitMinimum, PANE_MIN } from "../monitor/detail-width";
  import Stage from "../shell/Stage.svelte";
  import Reference from "../shell/Reference.svelte";
  import { composing } from "../shell/keys";
  import { targetSummary } from "./model-providers";
  import { activity } from "./tool-line";
  import { places } from "../shell/find";
  import ChatActivity from "./ChatActivity.svelte";
  import ChatMarkdown from "./ChatMarkdown.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import CurrentView from "./CurrentView.svelte";
  import ScanState from "./ScanState.svelte";

  const app = getAppState();
  const { current, chat: c } = app;
  /** The gap between the floating chat and the graph's bottom. */
  const FLOAT_GAP = 12;

  const branches = $derived(c.branches ?? []);
  /** The sessions grouped by project, the projects and their sessions newest first. */
  const projects = $derived.by(() => {
    const byProject: [string, ChatSession[]][] = [];
    for (const session of c.sessions) {
      const group = byProject.find(([name]) => name === session.project);
      if (group) group[1].push(session);
      else byProject.push([session.project, [session]]);
    }
    return byProject;
  });
  const view = $derived(c.view);
  const session = $derived(view?.session ?? null);
  const turns = $derived((view?.turns ?? []).map((turn) => ({ ...turn, activity: activity(turn.blocks) })));
  type Shown = (typeof turns)[number];
  const graph = $derived(session?.view === "graph");
  const mainMin = $derived(graph ? mainSplitMinimum(app.windowWidth) : 30);
  const live = $derived(graph && session?.unavailable === null);
  const mainNotice = $derived((live ? c.graph?.graph.error : null) ?? c.error);
  const running = $derived((view?.activity ?? "idle") !== "idle");
  const stopping = $derived(session !== null && c.stopping.has(session.id));
  const submitting = $derived(session !== null && (c.sending.has(session.id) || c.choosing.has(session.id)));
  const rejected = $derived(session === null ? undefined : c.rejected.get(session.id));
  const away = $derived(session !== null && c.scroll[session.id]?.following === false);
  const typed = $derived(session ? c.composer(session.id)?.shown ?? "" : "");
  /** The target New chat's session starts with, once the library said it for the chosen branch. */
  const answer = $derived(c.starting !== null && c.starting.worktree === c.worktree ? c.starting : null);
  const starting = $derived(answer?.target ?? null);
  const reading = $derived(answer === null || !answer.known);
  /** The shown session's reply card in Graph: the turn sent from Graph, while it is read. */
  const reply = $derived.by(() => {
    const card = session ? c.replies.get(session.id) : undefined;
    const turn = card ? turns[card.at] : undefined;
    return card && turn ? { at: card.at, folded: card.folded, turn } : null;
  });
  /** Which place of the conversation's matches the find field shows. */
  const shownPlace = $derived(c.found === 0 || !c.find ? 0 : ((c.find.at % c.found) + c.found) % c.found);

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
    if (item.activity === "waitingPermission") return "waiting";
    if (item.outcome === "running" || item.activity === "running") return "running";
    return item.unseen ? (item.outcome === "completed" ? "finished" : item.outcome) : null;
  }

  function planStepIcon(status: "pending" | "in_progress" | "completed" | "failed") {
    switch (status) {
      case "completed":
        return "green";
      case "in_progress":
        return "running";
      case "failed":
        return "red";
      default:
        return "queued";
    }
  }

  function permissionSettledLabel(
    status: "pending" | "selected" | "delivering" | "delivered" | "cancelled" | "expired",
    selectedOptionId: string | null,
    options: { optionId: string; name: string }[],
  ): string {
    if (status === "delivered" || status === "delivering" || status === "selected") {
      const picked = options.find((opt) => opt.optionId === selectedOptionId)?.name;
      return picked ? `Chosen: ${picked}` : "Submitted";
    }
    if (status === "cancelled") return "Cancelled";
    if (status === "expired") return "Expired";
    return "Waiting";
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

  /** The scroll position this component last placed on the mounted conversation element, so a
   * streaming update arriving before the browser's async scroll event cannot snap a user's
   * scroll back to the end. */
  let placed: { log: HTMLElement; top: number } | null = null;

  /** Read the same place again after a view or session switch; only follow at the end. */
  function remember(log: HTMLElement) {
    const id = session?.id;
    if (!id || app.mode !== "chat") return;
    const top = log.scrollTop;
    placed = { log, top };
    // A programmatic reveal may already be at the bottom; only an actual scroll or the button
    // should end that reading position.
    if (c.scroll[id]?.top === top) return;
    c.scroll[id] = { top, following: log.scrollHeight - log.clientHeight - top <= 2 };
  }

  function scrollToEnd(log: HTMLElement, id: string) {
    log.scrollTop = log.scrollHeight;
    placed = { log, top: log.scrollTop };
    c.scroll[id] = { top: log.scrollTop, following: true };
  }

  /** The floating chat's height, followed as the card folds, unfolds or grows and the composer
   * grows: the graph's camera keeps its Artifacts out of that band. */
  const banding: Attachment<HTMLElement> = (float) => {
    const measure = () => (c.band = Math.ceil(float.getBoundingClientRect().height) + FLOAT_GAP);
    const sized = new ResizeObserver(measure);
    sized.observe(float);
    measure();
    return () => {
      sized.disconnect();
      c.band = 0;
    };
  };

  /** The reply card follows its newest line as the reply streams. */
  const followReply: Attachment<HTMLElement> = (body) => {
    void reply?.turn;
    body.scrollTop = body.scrollHeight;
  };

  /** The place last brought into view, so a streaming turn does not move the reader again. */
  let lastShown = "";
  /** Find in the conversation (J-CHAT-FIND): mark every place the text appears, through the
   * CSS highlights so the conversation's own nodes stay as drawn, and bring the shown one into
   * view when it changes. Re-marks as the conversation streams. */
  const marking: Attachment<HTMLElement> = (log) => {
    const find = c.find;
    void view?.turns;
    const highlights = typeof CSS !== "undefined" && "highlights" in CSS ? CSS.highlights : null;
    const text = find && find.text.trim() !== "" ? find.text.toLowerCase() : "";
    if (!find || text === "") {
      c.found = 0;
      lastShown = "";
      return;
    }
    const ranges = places(log, text);
    c.found = ranges.length;
    if (ranges.length === 0) return;
    const at = ((find.at % ranges.length) + ranges.length) % ranges.length;
    if (highlights) {
      highlights.set("chat-find", new Highlight(...ranges));
      highlights.set("chat-find-current", new Highlight(ranges[at]));
    }
    const key = `${text}#${at}`;
    if (key !== lastShown) {
      lastShown = key;
      ranges[at].startContainer.parentElement?.scrollIntoView?.({ block: "center" });
    }
    return () => {
      highlights?.delete("chat-find");
      highlights?.delete("chat-find-current");
    };
  };

  /** Enter shows the next place, Shift+Enter the one before; Esc closes the field. */
  function findKey(event: KeyboardEvent) {
    const find = c.find;
    if (!find || composing(event)) return;
    if (event.key === "Enter") {
      event.preventDefault();
      c.find = { text: find.text, at: find.at + (event.shiftKey ? -1 : 1) };
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      c.find = null;
      c.panes.main?.focus({ preventScroll: true });
    }
  }

  /** Esc in the Pane's search: every session listed again, and the top row back. */
  async function closeSearch() {
    c.search("");
    c.searching = false;
    await tick();
    c.panes.searchTool?.focus();
  }

  const follow: Attachment<HTMLElement> = (log) => {
    const id = session?.id;
    const reveal = c.reveal;
    const turns = view?.turns; // Recheck when a library snapshot brings a new line or Run card.
    if (!id || !turns || app.mode !== "chat") {
      placed = null;
      return;
    }
    let attached = true;
    function place() {
      if (!attached || !id || app.mode !== "chat") return;
      const card = reveal && [...log.querySelectorAll<HTMLElement>("[data-run]")].find((item) => item.dataset.run === reveal);
      if (card && c.reveal === reveal) {
        card.scrollIntoView({ block: "center" });
        placed = { log, top: log.scrollTop };
        c.scroll[id] = { top: log.scrollTop, following: false };
        c.reveal = null;
      } else if (!c.reveal) {
        if (placed !== null && placed.log === log && log.scrollTop !== placed.top) {
          remember(log);
          return;
        }
        const kept = c.scroll[id];
        const following = !kept || kept.following;
        log.scrollTop = following ? log.scrollHeight : kept.top;
        placed = { log, top: log.scrollTop };
        if (following) c.scroll[id] = { top: log.scrollTop, following: true };
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

<!-- What the agent did in a turn and how it ended, in the conversation and in the reply card. -->
{#snippet turnBody(turn: Shown)}
  {@const turnActive = turn.state.kind === "running" || turn.state.kind === "waitingPermission"}
  {#each turn.activity as part (part.at)}
    {#if part.kind === "group"}
      <ChatActivity tools={part.tools} grouped running={turnActive} />
    {:else if part.block.kind === "text"}
      <ChatMarkdown text={part.block.text} />
    {:else if part.block.kind === "thought"}
      <details class="thought">
        <summary><Icon name="fold" />Thought</summary>
        <div class="thought-body"><ChatMarkdown text={part.block.text} /></div>
      </details>
    {:else if part.block.kind === "tool"}
      <ChatActivity tools={[part.block]} running={turnActive} />
    {:else if part.block.kind === "plan"}
      <div class="plan" role="region" aria-label="Agent plan">
        <p class="plan-label">Plan</p>
        <ol>
          {#each part.block.steps as step, stepIndex (stepIndex)}
            <li class:done={step.status === "completed"} class:failed={step.status === "failed"}>
              <span class="step-icon" class:tone-running={step.status === "in_progress"}>
                <Icon name={planStepIcon(step.status)} />
              </span>
              <span>{step.content}</span>
            </li>
          {/each}
        </ol>
      </div>
    {:else if part.block.kind === "permission"}
      {@const perm = part.block}
      <div class="permission" class:pending={perm.status === "pending"} data-permission={perm.permissionId} role="region" aria-label="Permission request: {perm.title}">
        <div class="perm-head">
          <span class="perm-title">{perm.title}</span>
          <span class="perm-status" class:waiting={perm.status === "pending"}>
            {permissionSettledLabel(perm.status, perm.selectedOptionId, perm.options)}
          </span>
        </div>
        {#if perm.summary}<p class="perm-summary mono">{perm.summary}</p>{/if}
        {#if perm.detail}<pre class="perm-detail">{perm.detail}</pre>{/if}
        {#if perm.status === "pending" && session}
          {@const sid = session.id}
          {@const busy = c.respondingPermission.has(perm.permissionId)}
          <div class="perm-actions">
            {#each perm.options as option (option.optionId)}
              <button
                type="button"
                class="button small"
                class:primary={option.kind === "allow_once"}
                disabled={busy}
                onclick={() => void c.respondPermission(sid, perm.permissionId, option.optionId)}
              >
                <span>{option.name}</span>
                {#if option.persistsInAgentSettings}
                  <span class="perm-persist">May be saved in agent settings</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if part.block.kind === "notice"}
      <p class="notice-block" role="status">{part.block.text}</p>
    {:else if part.block.kind === "handoff"}
      <details class="handoff">
        <summary><Icon name="fold" />Continued from {part.block.from} to {part.block.to}</summary>
        <pre class="handoff-summary">{part.block.summary}</pre>
      </details>
    {:else}
      {@render runCard(part.block.card)}
    {/if}
  {/each}
  {#if turn.state.kind === "stopped"}
    <p class="end">Stopped</p>
  {:else if turn.state.kind === "interrupted"}
    <p class="end">Interrupted · The app or agent process exited before the turn finished.</p>
  {:else if turn.state.kind === "failed"}
    <p class="end failed">{turn.state.message}</p>
  {/if}
{/snippet}

<!-- The composer: one field, the target picker at its bottom left and Send at its bottom right. -->
{#snippet composer(current: ChatSession)}
  {@const canSendTarget = current.target !== null && current.target.engine !== "legacyClaudeAgent"}
  <div class="composer">
    {#if !graph && current.unavailable}
      <!-- The session's branch cannot be worked on now: it says why, and sends nothing. -->
      <p class="unavailable" role="status">{current.unavailable}</p>
    {/if}
    {#if current.legacyMigrationNotice}
      <div class="legacy-banner" role="status">
        {#if current.target?.engine === "legacyClaudeAgent"}
          <span>Legacy Claude Agent Chat has been retired. Choose <strong>Claude ACP</strong> or a direct provider below to continue this conversation with an automatic handoff.</span>
        {:else}
          <span>Migrated from legacy Antigravity Chat to the unified ACP engine; sending continues with a deterministic handoff.</span>
        {/if}
      </div>
    {/if}
    {#if rejected}
      <div class="rejected">
        <span class="failed" role="status">{rejected.reason}</span>
        <button class="button small" onclick={() => void c.copy(rejected.text)}>Copy</button>
        <button class="button small" disabled={typed !== ""} onclick={() => { c.restore(current.id); c.panes.composer?.focus({ preventScroll: true }); }}>Restore</button>
      </div>
    {/if}
    <div class="box" class:off={current.unavailable !== null} class:pointer-edit={c.composerPointer}>
      <textarea
        class="bare-field"
        rows="2"
        aria-label="Message"
        disabled={current.unavailable !== null}
        placeholder="Ask the agent to work on this branch…"
        bind:this={c.panes.composer}
        value={typed}
        onpointerdown={() => (c.composerPointer = true)}
        onblur={() => (c.composerPointer = false)}
        oninput={(event) => write(event.currentTarget.value)}
        onkeydown={composerKey}
      ></textarea>
      <div class="row">
        <ModelPicker session={current} />
        <span class="spacer"></span>
        {#if stopping}
          <span class="stopping" role="status"><span class="tone-running"><Icon name="running" /></span>Stopping…</span>
        {:else if running}
          <button class="button icon-button" disabled={current.unavailable !== null} aria-label="Stop (Esc)" aria-keyshortcuts="Escape" onclick={() => void c.stop()}><Icon name="stop" /></button>
        {:else}
          {#if !canSendTarget}<span class="need-model" id="chat-need-model">{current.legacyMigrationNotice ? "Switch target to send" : "Choose a model to send"}</span>{/if}
          <button class="button icon-button primary" aria-label="Send (Enter)" aria-describedby={canSendTarget ? undefined : "chat-need-model"} aria-keyshortcuts="Enter" disabled={!canSendTarget || typed.trim() === "" || current.unavailable !== null || submitting} onclick={() => void c.send()}><Icon name="send" /></button>
        {/if}
      </div>
    </div>
  </div>
{/snippet}

<!-- The chat floating over the graph's bottom: the reply card, then the composer (D-CUR-10). -->
{#snippet floatingChat(current: ChatSession)}
  <div class="float" role="group" aria-label="Chat" {@attach banding}>
    {#if reply}
      {@const card = reply}
      <section class="reply" aria-label="Reply">
        <button class="reply-head" aria-expanded={!card.folded} onclick={() => c.replies.set(current.id, { at: card.at, folded: !card.folded })}>
          <Icon name={card.folded ? "fold" : "unfold"} />
          <span class="said">{card.turn.user}</span>
          {#if card.turn.state.kind === "running"}<span class="tone-running" aria-label="Running"><Icon name="running" /></span>{/if}
        </button>
        {#if !card.folded}
          <div class="reply-body" {@attach followReply}>{@render turnBody(card.turn)}</div>
        {/if}
      </section>
    {/if}
    {@render composer(current)}
  </div>
{/snippet}

<!-- The scan state of the Current shown, in the stage's status line. -->
{#snippet scan()}
  {#if session}<ScanState branch={session.branchKey} />{/if}
{/snippet}

<div class="mode" {@attach following}>
  <PaneGroup direction="horizontal" autoSaveId="chat-layout" class="columns">
    {#if app.paneShown}
      <Split id="chat-pane" order={1} defaultSize={20} minSize={PANE_MIN}>
        <section class="pane" aria-label="Chats" bind:this={c.panes.pane}>
          <div class="top">
            {#if c.searching || c.query !== ""}
              <label class="search">
                <Icon name="find" />
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  type="search"
                  value={c.query}
                  bind:this={c.panes.search}
                  aria-label="Search chats"
                  placeholder="Search chats"
                  autofocus
                  oninput={(event) => c.search(event.currentTarget.value)}
                  onblur={(event) => { if (event.currentTarget.value === "") c.searching = false; }}
                  onkeydown={(event) => {
                    if (event.key === "Escape" && !composing(event)) {
                      event.preventDefault();
                      void closeSearch();
                    }
                  }}
                />
              </label>
            {:else}
              <button class="row-button new" onclick={() => app.newChat()}><span class="lead"><Icon name="new-chat" /></span>New chat</button>
              <button class="row-button tool" aria-label="Search chats" title="Search chats" bind:this={c.panes.searchTool} onclick={() => (c.searching = true)}><Icon name="find" /></button>
            {/if}
          </div>
          <div class="sessions">
            {#if c.listed && c.query.trim() && projects.length === 0}<p class="empty">No chat matches.</p>{/if}
            {#if projects.length > 0}<p class="group">Projects</p>{/if}
            {#each projects as [project, members] (project)}
              {@const open = c.query.trim() !== "" || !c.folded.has(project)}
              <div class="project-row">
                <button class="row-button project" aria-expanded={open} onclick={() => (c.folded.has(project) ? c.folded.delete(project) : c.folded.add(project))}>
                  <span class="lead"><Icon name={open ? "folder-open" : "folder-closed"} /></span><span class="name">{project}</span>
                </button>
                <button class="row-button new-here" aria-label="New chat in {project}" title="New chat in {project}" onclick={() => void c.newChat(members[0].branchKey, true, project)}><Icon name="new-chat" /></button>
              </div>
              {#if open}
                <ul aria-label="Sessions of {project}">
                  {#each members as item (item.id)}
                    {@const state = sessionState(item)}
                    <li>
                      <button class="row-button session" class:selected={c.selected === item.id} aria-current={c.selected === item.id ? "true" : undefined} onclick={() => void c.open(item.id)}>
                        <span class="title">{item.title || "New chat"}</span>
                        {#if item.branch}<span class="branch">{item.branch}</span>{/if}
                        <span class="spacer"></span>
                        <span class="age" class:state class:running={state === "running"} class:waiting={state === "waiting"} class:failed={state === "failed"} class:finished={state === "finished"}>{state ?? ages.get(item.id)}</span>
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            {/each}
          </div>
        </section>
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="chat-main" order={2} defaultSize={app.paneShown ? 80 : 100} minSize={mainMin}>
      <Stage label="Chat's main area" notice={mainNotice} bind:noticeOpen={c.noticeOpen} bind:element={c.panes.main} status={live ? scan : undefined}>
        {#snippet tabs()}
          {#if session}
            <div class="views" role="tablist" aria-label="Session view">
              <button type="button" role="tab" aria-selected={!graph} onclick={() => void c.setView("conversation")}>Conversation</button>
              <button type="button" role="tab" aria-selected={graph} onclick={() => void c.setView("graph")}>Graph</button>
            </div>
          {/if}
        {/snippet}
        {#snippet tools()}
          {#if session && !graph}
            {#if c.find}
              {@const find = c.find}
              <label class="find">
                <Icon name="find" />
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  aria-label="Find in the conversation"
                  value={find.text}
                  autofocus
                  oninput={(event) => (c.find = { text: event.currentTarget.value, at: 0 })}
                  onblur={(event) => { if (event.currentTarget.value === "") c.find = null; }}
                  onkeydown={findKey}
                />
                {#if find.text.trim() !== ""}<span class="count" role="status">{c.found === 0 ? "No match" : `${shownPlace + 1} of ${c.found}`}</span>{/if}
              </label>
            {:else}
              <button class="tool" aria-label="Find (Ctrl+F)" title="Find" aria-keyshortcuts="Control+F" onclick={() => (c.find = { text: "", at: 0 })}><Icon name="find" /></button>
            {/if}
          {/if}
          {#if session && live}
            <button class="tool" aria-label="Refresh (Ctrl+R)" title="Refresh" aria-keyshortcuts="Control+R F5" onclick={() => void current.scan(session.branchKey, true)}>Refresh</button>
          {/if}
          {#if session}
            {@const label = running ? "Stop the running turn to move the chat" : "Move to another checkout"}
            <span class="tool-sep"></span>
            <button class="tool" aria-label="Move to another checkout" title={label} aria-expanded={c.moving !== null} disabled={running} onclick={() => void c.openMove()}><Icon name="move" /></button>
          {/if}
        {/snippet}
        {#if !session}
          <p class="empty">{c.listed && !c.query.trim() && c.sessions.length === 0 ? "Start a session with New chat." : "Choose a session in the Pane, or start one with New chat."}</p>
        {:else if graph}
          {#if session.unavailable !== null}
            <div class="graph-off">
              <p class="empty" role="status">{session.unavailable}</p>
              {@render floatingChat(session)}
            </div>
          {:else if c.graph}
            {@const shownSession = session}
            <!-- A moved session shows its new branch's Current. -->
            {#key `${session.id} ${keyOf(session.branchKey)}`}
              <CurrentView branch={session.branchKey} worktree={session.worktree} workspace={c.graph} bottom={c.band}>
                {#snippet floating()}{@render floatingChat(shownSession)}{/snippet}
              </CurrentView>
            {/key}
          {/if}
        {:else}
          <div class="chat">
            {#key session.id}
              <div class="reading">
                <div class="convo" role="log" aria-label="Conversation" {@attach follow} {@attach marking} onscroll={(event) => remember(event.currentTarget)}>
                  <div class="column">
                    {#each turns as turn, at (at)}
                      <div class="message">{turn.user}</div>
                      <div class="turn">{@render turnBody(turn)}</div>
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
            {@render composer(session)}
          </div>
        {/if}
      </Stage>
    </Split>
  </PaneGroup>
</div>

<!-- Move chat: another checkout for the shown session (J-CHAT-MOVE). -->
<Dialog.Root open={c.moving !== null} onOpenChange={(open) => { if (!open) c.moving = null; }}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog move-chat">
      <Dialog.Title class="dialog-title">Move chat</Dialog.Title>
      {#if c.moving}
        {@const moving = c.moving}
        {@const own = session?.worktree ?? null}
        <form class="dialog-layout" onsubmit={(event) => { event.preventDefault(); void c.move(); }}>
          <div class="dialog-body">
            <label class="field">
              <span class="field-name">Checkout</span>
              <select value={moving.worktree} disabled={!moving.branches?.length} onchange={(event) => (c.moving = { ...moving, worktree: event.currentTarget.value, error: null })}>
                {#each moving.branches ?? [] as branch (branch.workspace)}
                  <option value={branch.workspace}>{branch.name ? `${branch.project} / ${branch.name}` : branch.project}</option>
                {/each}
              </select>
            </label>
            {#if moving.error}<p class="refused" role="alert">{moving.error}</p>{/if}
            <p class="starting quiet">The conversation stays; from its next turn the agent works in the chosen checkout, and Graph shows its Current.</p>
          </div>
          <div class="dialog-actions">
            <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
            <button class="button primary" type="submit" aria-keyshortcuts="Enter" disabled={moving.busy || moving.worktree === own || moving.branches === null}>Move</button>
          </div>
        </form>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

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
            <span class="field-name" id="new-chat-model">Agent or model</span>
            <p class="starting" class:quiet={!starting} role="status" aria-labelledby="new-chat-model">
              {#if c.branchError}
                {c.branchError}
              {:else if c.branches === null}
                Reading branches…
              {:else if branches.length === 0}
                No checked-out branch.
              {:else if c.worktree === null}
                Choose a branch.
              {:else if starting}
                {targetSummary(starting, c.agents.map((item) => item.entry))}
              {:else if reading}
                Checking available agents and providers…
              {:else}
                No agent or provider is ready. The session starts without one; set up a Chat agent or provider in Settings to send.
              {/if}
            </p>
          </div>
        </div>
        <div class="dialog-actions">
          <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
          <button class="button primary" type="submit" aria-keyshortcuts="Enter" disabled={!c.worktree || reading}>Create</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<!-- Trust workspace: explicit consent before spawning an ACP agent in a worktree (D-OVL-8). -->
<Dialog.Root open={c.trustPrompt !== null} onOpenChange={(open) => { if (!open && !c.trustPrompt?.busy) c.trustPrompt = null; }}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog trust-workspace">
      <Dialog.Title class="dialog-title">Trust workspace</Dialog.Title>
      {#if c.trustPrompt}
        {@const prompt = c.trustPrompt}
        {@const req = prompt.requirement}
        <form
          class="dialog-layout"
          onsubmit={(event) => {
            event.preventDefault();
            void c.confirmTrust();
          }}
        >
          <div class="dialog-body">
            <p class="starting quiet">{req.reason}</p>
            <dl class="trust-facts">
              <dt>Agent</dt>
              <dd>{req.agentName} (<span class="mono">{req.agentId}</span>)</dd>
              <dt>Scope</dt>
              <dd class="mono">{req.scopeLabel}</dd>
              <dt>Executable</dt>
              <dd class="mono">{req.fingerprint.resolvedExecutable}</dd>
              <dt>SHA-256</dt>
              <dd class="mono">{req.fingerprint.executableSha256}</dd>
            </dl>
            <p class="starting quiet">{req.disclosure}</p>
            {#if prompt.error}<p class="refused" role="alert">{prompt.error}</p>{/if}
          </div>
          <div class="dialog-actions">
            <Dialog.Close class="button" disabled={prompt.busy} aria-keyshortcuts="Escape">Cancel</Dialog.Close>
            <button class="button primary" type="submit" aria-keyshortcuts="Enter" disabled={prompt.busy}>
              {prompt.busy ? "Trusting…" : "Trust and continue"}
            </button>
          </div>
        </form>
      {/if}
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
  /* The sidebar (D-CHAT-1): the top row (New chat and Search), then each project a folder row
   * over its sessions, every row --row-pane high and chosen across the Pane's width. The
   * new-chat icon and the folders stand on one x, the sessions' titles on the next. */
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
  }
  .top {
    flex: none;
    display: flex;
    align-items: center;
    height: var(--row-pane);
    box-sizing: border-box;
  }
  /* The projects' quiet group label, straight under the top row (D-SYS-5). */
  .group {
    margin: 0;
    padding: 6px var(--pad) 4px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  /* A project's row: the folder row, and New chat in its last 36px cell while the pointer or
   * the keyboard is on the row. */
  .project-row {
    position: relative;
  }
  .new-here {
    position: absolute;
    top: 0;
    right: 0;
    width: var(--row-pane);
    padding: 0;
    justify-content: center;
    opacity: 0;
  }
  .project-row:hover .new-here,
  .project-row:focus-within .new-here {
    opacity: 1;
  }
  .new-here:hover {
    color: var(--ink);
  }
  .project-row:hover .project {
    background: var(--surface-group);
  }
  .refused {
    margin: 0;
    color: var(--red);
    font-size: var(--text-small);
  }
  /* A row of the Pane: chosen by a click anywhere on it, lit whole by the pointer, the
   * keyboard's ring inside it. */
  .row-button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row-pane);
    min-width: 0;
    margin: 0;
    padding: 0 var(--pad);
    box-sizing: border-box;
    border: 0;
    border-radius: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row-button:not(.selected):hover {
    background: var(--surface-group);
  }
  .row-button:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .lead {
    display: inline-flex;
    flex: none;
    color: var(--ink-2);
  }
  .top .new {
    flex: 1;
    color: var(--ink);
  }
  /* Search: the find icon alone in the row's last 36px cell. */
  .top .tool {
    flex: none;
    justify-content: center;
    width: var(--row-pane);
    padding: 0;
  }
  .top .tool:hover {
    color: var(--ink);
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: var(--control);
    margin: 0 8px 0 4px;
    padding: 0 7px;
    box-sizing: border-box;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink-2);
  }
  .search:focus-within {
    outline: 1px solid var(--current-mark);
    outline-offset: -2px;
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    outline: none;
    background: none;
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
  .project {
    color: var(--label);
  }
  .project .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A session: its title on the second x, its branch quieter after it, its age or state at the
   * right. The branch gives way first, down to its first ten characters; then the title. */
  .session {
    padding-left: calc(var(--pad) + var(--icon) + 8px);
  }
  .session.selected {
    background: var(--selection);
  }
  .session .title {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .session.selected .title {
    color: var(--ink);
  }
  .session .branch {
    flex: 0 1000 auto;
    min-width: 10ch;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
    font: var(--text-label) var(--mono);
  }
  .session .spacer {
    flex: 1;
    min-width: 0;
  }
  .age {
    flex: none;
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }
  .age.running { color: var(--running); }
  .age.failed { color: var(--red); }
  .age.finished { color: var(--ink); }
  /* The view tabs (D-CHAT-10): two words, the shown one bright over the current mark. */
  .views {
    display: flex;
    gap: 16px;
    height: 100%;
  }
  .views button {
    height: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: none;
    color: var(--ink-off);
    font: inherit;
    font-weight: 400;
    cursor: pointer;
  }
  .views button:hover {
    color: var(--ink);
  }
  .views button[aria-selected="true"] {
    color: var(--ink-bright);
    box-shadow: inset 0 -2px 0 var(--current-mark);
  }
  .views button:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  /* Find in the conversation: the find tool's field in its place (D-CHAT-11). */
  .find {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 240px;
    max-width: 100%;
    height: var(--control);
    padding: 0 7px;
    box-sizing: border-box;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink-2);
  }
  .find:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .find input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--ink);
    font: inherit;
  }
  .find .count {
    flex: none;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
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
    background: var(--surface-app);
  }
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
  .box:not(.pointer-edit):has(textarea:focus-visible) {
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
  .need-model { min-width: 0; color: var(--ink-2); font-size: var(--text-small); }
  .stopping .tone-running { display: inline-flex; }
  .icon-button {
    display: inline-grid;
    place-items: center;
    width: var(--control);
    padding: 0;
  }
  /* The chat over the graph (D-CUR-10): the reply card and the composer at the canvas's bottom,
   * centred in the canvas left of the Detail, the conversation column's width; a dark face
   * inside a thin edge, no shadow. */
  .float {
    position: absolute;
    z-index: 15;
    left: 0;
    right: var(--detail-cover, 0px);
    bottom: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: var(--measure);
    margin: 0 auto;
    padding: 0 var(--pad);
    box-sizing: border-box;
  }
  .float .composer {
    max-width: none;
    margin: 0;
    padding: 0;
  }
  .reply {
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    background: var(--surface-input);
  }
  .reply-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row-head);
    margin: 0;
    padding: 0 8px;
    box-sizing: border-box;
    border: 0;
    border-radius: var(--radius-chip);
    background: none;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    cursor: pointer;
  }
  .reply-head:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .reply-head .said {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reply-head .tone-running {
    display: inline-flex;
  }
  .reply-body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 120px;
    overflow: auto;
    padding: 0 12px 8px 28px;
    color: var(--ink);
  }
  /* A session that cannot be worked on in Graph: one quiet line in the graph's place. */
  .graph-off {
    position: relative;
    height: 100%;
    background: var(--surface-app);
  }
  .graph-off .empty {
    padding-top: 8px;
  }
  :global(.dialog.new-chat),
  :global(.dialog.move-chat) {
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
  .age.waiting { color: var(--yellow); }
  .thought, .handoff {
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .thought > summary, .handoff > summary {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    list-style: none;
    min-height: var(--row-dense);
  }
  .thought > summary::-webkit-details-marker, .handoff > summary::-webkit-details-marker { display: none; }
  .thought-body, .handoff-summary {
    margin: 4px 0 0 18px;
    padding: 6px 8px;
    border-left: 2px solid var(--line-group);
    color: var(--ink-2);
  }
  .handoff-summary {
    font-family: var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .plan {
    padding: 6px 10px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    font-size: var(--text-small);
  }
  .plan-label {
    margin: 0 0 4px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .plan ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .plan li { display: flex; align-items: center; gap: 6px; }
  .plan li.done { color: var(--ink-2); }
  .plan li.failed { color: var(--red); }
  .step-icon { display: inline-flex; flex: none; color: var(--ink-3); }
  .permission {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    font-size: var(--text-small);
  }
  .perm-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .perm-title { font-weight: 600; color: var(--ink); }
  .perm-status { color: var(--ink-2); }
  .perm-status.waiting { color: var(--yellow); font-weight: 500; }
  .perm-summary { margin: 0; color: var(--ink-2); overflow-wrap: anywhere; }
  .perm-detail {
    margin: 0;
    padding: 6px 8px;
    max-height: 240px;
    overflow: auto;
    background: var(--surface-input);
    border-radius: var(--radius-control);
    font-family: var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .perm-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .perm-actions .button { display: inline-flex; flex-direction: column; align-items: flex-start; height: auto; padding: 4px 10px; }
  .perm-persist { font-size: var(--text-label); color: var(--ink-2); font-weight: 400; }
  .notice-block { margin: 0; color: var(--ink-2); font-size: var(--text-small); }
  .legacy-banner {
    padding: 6px 10px;
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    color: var(--ink);
    font-size: var(--text-small);
  }
  :global(.dialog.trust-workspace) { width: min(480px, 92vw); }
  .trust-facts {
    display: grid;
    grid-template-columns: 92px minmax(0, 1fr);
    gap: 4px 10px;
    margin: 8px 0;
    font-size: var(--text-small);
  }
  .trust-facts dt { color: var(--ink-2); }
  .trust-facts dd { margin: 0; overflow-wrap: anywhere; }
</style>
