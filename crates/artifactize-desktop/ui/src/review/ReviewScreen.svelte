<script lang="ts">
  // The standalone review: a search over saved requests (by default the waiting Human
  // sign-offs), the shared Human Detail of a Human request and a read-only Detail of any other.
  import { ContextMenu, Dialog } from "bits-ui";
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import { getAppState } from "../shell/app.svelte";
  import { reviewRefreshed } from "../ipc/activity.svelte";
  import type { Attachment } from "svelte/attachments";
  import {
    call,
    useIpc,
    message,
    onStateChanged,
    type LaunchView,
    type Listed,
    type Results,
  } from "../ipc/client";
  import HumanDetail from "./HumanDetail.svelte";
  import SearchField from "./SearchField.svelte";
  import RecordDetail from "../monitor/RecordDetail.svelte";
  import Sections from "../monitor/Sections.svelte";
  import Panel from "../shell/Panel.svelte";
  import { ReviewScreenState, setReviewScreenState } from "./review-screen-state.svelte";
  import { scrollTo } from "../shell/attach";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let { launch, active }: { launch: LaunchView; active: boolean } = $props();
  const commands = useIpc();
  // The launch is fixed for the window's life: its search is where the screen starts.
  // svelte-ignore state_referenced_locally
  const r = new ReviewScreenState(launch.search);
  setReviewScreenState(r);
  const app = getAppState();

  /** The search every launch starts from: waiting Human reviews, of `--repo` when given. */
  const WAITING = "kind:human status:waiting_human";

  /** What is shown of the last search answer is derived from it. */
  const rows = $derived(r.results?.rows ?? []);
  const next = $derived(r.results?.next ?? null);
  const waiting = $derived(r.results?.waiting ?? true);
  const invalid = $derived(r.results?.error ?? null);
  const loaded = $derived(r.results !== null);

  const selected = $derived(rows[r.index] ?? null);
  const page = $derived(r.previous.length + 1);
  const scope = $derived(r.query.trim() === "" ? "all requests" : r.query.trim() === WAITING ? "waiting Human reviews" : r.query.trim());

  function load(reload = true) {
    return request(reload).shown;
  }

  /** Ask for the list: `answer` is this read's own, `shown` settles once the list shows the
   * newest answer (an overtaken read waits for the read that overtook it). */
  function request(reload: boolean) {
    const sequence = ++r.reads;
    const answer = call(commands.reviewSearch(r.query, r.start, reload));
    r.loading = read(sequence, reload, answer);
    return { answer, shown: r.loading };
  }

  async function read(sequence: number, reload: boolean, answer: Promise<Results>): Promise<void> {
    try {
      const found = await answer;
      // A refresh can overtake a search: its waiter needs the newest answer, not this stale one.
      if (sequence !== r.reads) return r.loading;
      // The state generation the list and the open review were read from.
      const generation = r.results?.generation ?? null;
      if (generation !== null && found.generation !== generation) {
        // The state was replaced: the open review, its drafts and the Preview are of the old one.
        r.review = null;
        app.forgetReviews();
        r.record = null;
        r.preview = null;
        r.focus = "list";
      }
      const id = selected?.id;
      r.results = found;
      r.error = null;
      const kept = id ? rows.findIndex((row) => row.id === id) : -1;
      r.index = kept >= 0 ? kept : Math.min(r.index, Math.max(rows.length - 1, 0));
      if (reload && r.record?.request) void reopen(r.record.request);
    } catch (failure) {
      if (sequence !== r.reads) return r.loading;
      r.error = message(failure);
    }
  }

  /** The manual refresh of the list (`r`, the palette's "Refresh the list"). */
  function refresh() {
    const { answer, shown } = request(true);
    // E2E builds only: witness this refresh once its own reload succeeded and the list then
    // settled on screen, whichever read it shows; what it shows is for the reader to check.
    // Its own failure, or no answer yet, is never witnessed, whatever other reads answered.
    if (import.meta.env.MODE === "e2e") {
      void Promise.all([answer, shown]).then(
        async () => {
          await tick();
          reviewRefreshed(app.activity, r.query, r.start);
        },
        () => {},
      );
    }
  }

  /** Every change of the query, typed or set by a command: a pause in typing searches. */
  function setQuery(text: string) {
    r.query = text;
    clearTimeout(r.typing);
    if (text !== r.searched) r.typing = setTimeout(() => void research(), 120);
  }

  /** Search from the newest page, or await the answer already requested for this query. */
  async function research() {
    if (r.query === r.searched) return r.loading;
    r.searched = r.query;
    r.start = 0;
    r.previous = [];
    r.index = 0;
    await load();
  }

  function older() {
    if (next === null) return;
    r.previous = [...r.previous, r.start];
    r.start = next;
    r.index = 0;
    void load();
  }

  function newer() {
    if (r.previous.length === 0) return;
    r.start = r.previous[r.previous.length - 1];
    r.previous = r.previous.slice(0, -1);
    r.index = 0;
    void load();
  }

  onMount(() => {
    void load();
    if (launch.request) void openId(launch.request);
    const clock = setInterval(() => void load(false), 1000);
    const stop = onStateChanged(() => void load());
    return () => {
      clearInterval(clock);
      clearTimeout(r.typing);
      stop();
    };
  });

  /** The read-only Preview of the selected request; opening it claims nothing. Every list
   * read (and clock redraw) shows it again from the same snapshot as the list. */
  const previewSelected: Attachment<HTMLElement> = () => {
    void rows;
    const row = selected;
    if (!row || r.focus !== "list") return;
    let current = true;
    const shown =
      row.kind === "human"
        ? call(commands.humanOpen(row.id, false)).then((human) => ({ human }))
        : call(commands.reviewDetail(row.id)).then((record) => ({ record }));
    shown.then(
      (view) => current && (r.preview = view),
      () => current && (r.preview = null),
    );
    return () => {
      current = false;
    };
  };

  /** Open a Human request; another request than the open one starts from a clean review. */
  function open(id: string, tools = false) {
    r.review = app.review(id);
    if (tools) r.review.area = "tools";
    r.record = null;
    r.focus = "detail";
    r.notice = null;
  }

  /** Read the read-only Detail of `id` again, keeping its toggles. */
  async function reopen(id: string) {
    try {
      const view = await call(commands.reviewDetail(id));
      if (r.record?.request === id) r.record = view;
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** Open the selected row: a Human request for review, any other read-only. */
  function openRow(row: Listed, tools = false) {
    if (row.kind === "human") return open(row.id, tools);
    void openId(row.id);
  }

  /** Open a request by ID in any repository; a waiting follower's ID opens its original. */
  async function openId(id: string) {
    try {
      const view = await call(commands.reviewDetail(id));
      if (view.profile === "human") return open(id);
      r.review = null;
      if (r.record?.request !== view.request) {
        r.technical = false;
        r.pending = false;
        r.details = false;
      }
      r.record = view;
      r.focus = "detail";
      r.notice = null;
      void focusDetail();
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** Put focus in the open Detail, so its keys reach it (a reopened request keeps its Detail). */
  async function focusDetail() {
    await tick();
    const target = r.detailPane?.querySelector<HTMLElement>('[aria-label="Human review"], [role="document"]') ?? r.detailPane;
    if (target && !target.contains(document.activeElement)) target.focus();
  }

  async function back() {
    r.focus = "list";
    r.record = null;
    await tick();
    r.list?.focus();
    void load();
  }

  async function search() {
    r.focus = "list";
    r.record = null;
    await tick();
    r.field?.focus();
    r.field?.select();
  }

  /** Leave the search for the list, which first shows what the query finds. */
  async function leave() {
    await research();
    await tick();
    r.list?.focus();
  }

  /** Screen-wide: search works even when an empty Requests list cannot take focus. */
  function global(event: KeyboardEvent) {
    if (!active || event.defaultPrevented) return;
    if (chord(event, "o")) r.asking = true;
    else if (
      chord(event, "f") || (
        plain(event, "/") && r.focus === "list" &&
        !app.palette && !app.help && !r.asking && app.leaving.taken === null
      )
    ) void search();
    else return;
    event.preventDefault();
  }

  /** Showing the screen, or its first rows, puts focus on the list so its keys work at once;
   * focus already somewhere (the search field) stays there. */
  const takeFocus: Attachment<HTMLElement> = (element) => {
    if (!active || r.focus !== "list") return;
    void tick().then(() => {
      const current = document.activeElement;
      if (!current || current === document.body || !current.closest(".review")) element.focus();
    });
  };

  function keydown(event: KeyboardEvent) {
    if (r.focus === "detail" && r.record) return recordKey(event);
    if (r.focus !== "list") return;
    if (plain(event, "ArrowDown") || plain(event, "j")) r.index = Math.min(r.index + 1, rows.length - 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) r.index = Math.max(r.index - 1, 0);
    else if ((plain(event, "Enter") || plain(event, "ArrowRight")) && selected) openRow(selected);
    else if (plain(event, "PageDown")) older();
    else if (plain(event, "PageUp")) newer();
    else if (plain(event, "r")) refresh();
    else return;
    event.preventDefault();
  }

  function recordKey(event: KeyboardEvent) {
    const agent = r.record?.profile === "agent";
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) {
      if (r.details) r.details = false;
      else void back();
    } else if (plain(event, "t") && (!agent || r.details)) r.technical = !r.technical;
    else if (plain(event, "w") && (!agent || r.details)) r.pending = !r.pending;
    else if (plain(event, "d") && agent) r.details = !r.details;
    else return;
    event.preventDefault();
  }

  useCommands(() =>
    active
      ? [
          { id: "review.open-id", group: "Review", label: "Open a request by ID", keys: "Ctrl+O", run: () => (r.asking = true) },
          { id: "review.search", group: "Review", label: "Search requests", keys: "/", run: () => void search() },
          {
            id: "review.waiting",
            group: "Review",
            label: "Show waiting Human reviews",
            enabled: () => r.query !== launch.search,
            run: () => setQuery(launch.search),
          },
          { id: "review.refresh", group: "Review", label: "Refresh the list", keys: "r", run: refresh },
          { id: "review.older", group: "Review", label: "Older requests", keys: "PgDn", enabled: () => next !== null, run: older },
          { id: "review.newer", group: "Review", label: "Newer requests", keys: "PgUp", enabled: () => r.previous.length > 0, run: newer },
          {
            id: "review.tools",
            group: "Review",
            label: "Open the selected request with Tools focused",
            enabled: () => selected?.kind === "human",
            run: () => selected && open(selected.id, true),
          },
        ]
      : [],
  );
</script>

<svelte:window onkeydown={global} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="review" onkeydown={keydown} {@attach previewSelected}>
  <nav class="crumbs" aria-label="Breadcrumb">
    <span class="root">artifactize review</span>
    <span class="sep">›</span>
    <span class="scope">{scope}</span>
    {#if r.focus === "detail" && r.review?.view}
      <span class="sep">›</span><span>{r.review.view.title.split(" · ")[0]}</span>
    {:else if r.focus === "detail" && r.record}
      <span class="sep">›</span><span>{r.record.title.split(" · ")[0]}</span>
    {/if}
    {#if r.error || r.notice}<span class="notice" class:error={!!r.error}>{r.error ?? r.notice}</span>{/if}
  </nav>
  <PaneGroup direction="horizontal" autoSaveId="review-columns">
    <Pane defaultSize={38} minSize={20}>
      <Panel
        title="{waiting ? 'Waiting Human reviews' : 'Requests'} ({rows.length}){page > 1 || next !== null ? ` · page ${page}` : ''}"
        focused={r.focus === "list"}
        onfocus={() => (r.focus = "list")}
      >
        {#snippet actions()}
          {#if page > 1 || next !== null}
            <button class="button small" disabled={r.previous.length === 0} onclick={newer} title="Newer requests (PgUp)">‹ Newer</button>
            <button class="button small" disabled={next === null} onclick={older} title="Older requests (PgDn)">Older ›</button>
          {/if}
          <button class="button small" onclick={() => (r.asking = true)}>Open ID <kbd>Ctrl+O</kbd></button>
        {/snippet}
        <SearchField bind:value={() => r.query, setQuery} bind:input={r.field} complete={(text) => call(commands.reviewComplete(text))} onleave={() => void leave()} />
        {#if rows.length === 0}
          <p class="empty" class:error={!!invalid}>
            {invalid ?? (!loaded ? "Loading…" : waiting ? "No Human reviews are waiting." : "No request matches the search.")}
          </p>
        {:else}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              {#snippet child({ props })}
                <div {...props} class="waiting">
                  <div class="head" aria-hidden="true">
                    <span>Eval</span><span>Request</span><span>Status</span><span>Claim</span><span>Age</span><span>Repository</span>
                  </div>
                  <ul
                    role="listbox"
                    aria-label="Requests"
                    tabindex="0"
                    bind:this={r.list}
                    {@attach takeFocus}
                    {@attach scrollTo(() => `[data-index="${r.index}"]`)}
                  >
                    {#each rows as row, at (row.id)}
                      <li
                        role="option"
                        aria-selected={at === r.index}
                        class:selected={at === r.index}
                        data-index={at}
                        onclick={() => (at === r.index && r.focus === "list" ? openRow(row) : ((r.index = at), (r.focus = "list")))}
                        oncontextmenu={() => (r.index = at)}
                        ondblclick={() => openRow(row)}
                        onkeydown={() => {}}
                      >
                        <span><span class={tone(row.mark.status)}>{row.mark.glyph}</span> {row.eval}</span>
                        <span class="mono">{row.id}</span>
                        <span class={tone(row.mark.status)}>{row.mark.status}</span>
                        <span>{row.claim}</span>
                        <span>{row.age}</span>
                        <span class="repo">{row.repository}</span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/snippet}
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content class="menu">
                <ContextMenu.Item class="menu-item" onSelect={() => selected && openRow(selected)}>Open <kbd>Enter</kbd></ContextMenu.Item>
                {#if selected?.kind === "human"}
                  <ContextMenu.Item class="menu-item" onSelect={() => selected && open(selected.id, true)}>Open with Tools focused</ContextMenu.Item>
                {/if}
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        {/if}
      </Panel>
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={62} minSize={30}>
      <Panel
        title={r.focus === "detail" ? (r.record ? `${r.record.title} · read-only` : "Review") : "Preview"}
        focused={r.focus === "detail"}
        bind:element={r.detailPane}
        onfocus={() => (r.review || r.record) && (r.focus = "detail")}
      >
        {#snippet actions()}
          {#if r.focus === "detail" && r.record}
            {#if r.record.profile === "agent"}
              <button class="button small" onclick={() => (r.details = !r.details)} aria-pressed={r.details}>Details <kbd>d</kbd></button>
            {/if}
            <button class="button small" onclick={() => void back()} title="Back to the list">Close <kbd>Esc</kbd></button>
          {/if}
        {/snippet}
        {#if r.focus === "detail" && r.record}
          {#key r.record.request}
            <RecordDetail detail={r.record} bind:technical={r.technical} bind:pending={r.pending} bind:details={r.details} />
          {/key}
        {:else if r.focus === "detail" && r.review}
          {#key r.review}
            <HumanDetail review={r.review} {active} onback={() => void back()} onsubmitted={() => void load()} />
          {/key}
        {:else if r.preview && "human" in r.preview && selected}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="preview" role="button" tabindex="-1" onclick={() => openRow(selected)}>
            <h3>{r.preview.human.title}</h3>
            <p class="meta">{r.preview.human.meta}</p>
            {#if r.preview.human.shared}<p class="shared">{r.preview.human.shared}</p>{/if}
            <p class="instruction">{r.preview.human.instruction}</p>
            <p>
              <strong>Tools</strong>
              {r.preview.human.tools.length === 0
                ? "none declared in this eval's scope"
                : r.preview.human.tools.map((tool) => `${tool.name} (${tool.kind})`).join(" · ")}
            </p>
            <p class="hint">Enter or click: open review</p>
          </div>
        {:else if r.preview && "record" in r.preview && selected}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="preview" role="button" tabindex="-1" onclick={() => openRow(selected)}>
            <h3>{r.preview.record.title}</h3>
            {#if r.preview.record.summary}<p class="meta {tone(r.preview.record.mark?.status)}">{r.preview.record.summary}</p>{/if}
            <Sections sections={r.preview.record.sections} peek />
            <p class="hint">Enter or click: open read-only</p>
          </div>
        {:else}
          <p class="empty">Select a request.</p>
        {/if}
      </Panel>
    </Pane>
  </PaneGroup>
</div>

<Dialog.Root bind:open={r.asking}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content
      class="dialog"
      onCloseAutoFocus={(event) => {
        // An opened request takes focus, not what had it before the dialog.
        if (r.focus !== "detail") return;
        event.preventDefault();
        void focusDetail();
      }}
    >
      <Dialog.Title class="dialog-title">Open a request by ID</Dialog.Title>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (r.wanted.trim()) {
            void openId(r.wanted.trim());
            r.asking = false;
            r.wanted = "";
          }
        }}
      >
        <input class="input" placeholder="req-…" bind:value={r.wanted} aria-label="Request ID" />
        <div class="dialog-actions">
          <Dialog.Close class="button">Cancel</Dialog.Close>
          <button class="button primary" type="submit">Open</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .review {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100%;
  }
  .crumbs {
    display: flex;
    gap: 8px;
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--border);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
  }
  .root,
  .sep {
    color: var(--muted);
  }
  .notice {
    margin-left: auto;
    color: var(--muted);
    font-weight: 400;
  }
  .notice.error,
  .empty.error {
    color: var(--red);
  }
  .scope {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty,
  .hint {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .waiting {
    --columns: minmax(120px, 1.2fr) minmax(120px, 1fr) minmax(90px, 0.7fr) minmax(80px, 0.8fr) 64px minmax(80px, 1fr);
  }
  .head,
  li {
    display: grid;
    grid-template-columns: var(--columns);
    gap: 8px;
    padding: 5px var(--pad);
    white-space: nowrap;
  }
  .head span,
  li span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head {
    font-size: 11px;
    color: var(--muted);
    font-weight: 600;
    padding-top: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    outline: none;
  }
  li {
    cursor: pointer;
  }
  li:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  ul:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .mono {
    font: 11px var(--mono);
  }
  .repo {
    color: var(--muted);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .preview {
    padding: 0 var(--pad);
    cursor: pointer;
    outline: none;
  }
  .preview h3 {
    margin: 0 0 4px;
  }
  .meta {
    font-weight: 600;
    margin: 0 0 4px;
  }
  .shared {
    color: var(--human);
    margin: 0 0 4px;
  }
  .instruction {
    white-space: pre-wrap;
  }
  .input {
    width: 100%;
    font: inherit;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
  }
</style>
