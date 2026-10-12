<script lang="ts">
  // The standalone review: a search over saved requests (by default the waiting Human
  // sign-offs), the shared Human Detail of a Human request and a read-only Detail of any other.
  import { ContextMenu, Dialog } from "bits-ui";
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import {
    call,
    commands,
    message,
    onStateChanged,
    type DetailView,
    type HumanView,
    type LaunchView,
    type Listed,
    type Settled,
  } from "../ipc/client";
  import { Review } from "./review.svelte";
  import HumanDetail from "./HumanDetail.svelte";
  import SearchField from "./SearchField.svelte";
  import RecordDetail from "../monitor/RecordDetail.svelte";
  import Sections from "../monitor/Sections.svelte";
  import Panel from "../shell/Panel.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let { launch, active }: { launch: LaunchView; active: boolean } = $props();

  /** The search every launch starts from: waiting Human reviews, of `--repo` when given. */
  const WAITING = "kind:human status:waiting_human";

  // svelte-ignore state_referenced_locally
  let query = $state(launch.search);
  /** Where the shown page starts, and where the pages before it started. */
  let start = $state(0);
  let previous = $state<number[]>([]);
  let next = $state<number | null>(null);
  let waiting = $state(true);
  let rows = $state<Listed[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let invalid = $state<string | null>(null);
  let index = $state(0);
  let review = $state<Review | null>(null);
  /** The read-only Detail of an open request that is not a Human review. */
  let record = $state<DetailView | null>(null);
  let technical = $state(false);
  let pending = $state(false);
  let details = $state(false);
  let focus = $state<"list" | "detail">("list");
  let preview = $state<{ human: HumanView } | { record: DetailView } | null>(null);
  let asking = $state(false);
  let wanted = $state("");
  let list = $state<HTMLElement>();
  let field = $state<HTMLInputElement>();
  let detailPane = $state<HTMLElement>();
  let notice = $state<string | null>(null);
  /** The state generation the list and the open review were read from. */
  let generation: number | null = null;
  /** Each read supersedes the ones before it, so a slow answer never shows an older query. */
  let reads = 0;

  const selected = $derived(rows[index] ?? null);
  const page = $derived(previous.length + 1);
  const scope = $derived(query.trim() === "" ? "all requests" : query.trim() === WAITING ? "waiting Human reviews" : query.trim());

  async function load(reload = true) {
    const read = ++reads;
    try {
      const found = await call(commands.reviewSearch(query, start, reload));
      if (read !== reads) return;
      if (generation !== null && found.generation !== generation) {
        // The state was replaced: the open review, its drafts and the Preview are of the old one.
        review = null;
        record = null;
        preview = null;
        focus = "list";
      }
      generation = found.generation;
      const id = selected?.id;
      rows = found.rows;
      next = found.next;
      waiting = found.waiting;
      invalid = found.error;
      loaded = true;
      error = null;
      const kept = id ? rows.findIndex((row) => row.id === id) : -1;
      index = kept >= 0 ? kept : Math.min(index, Math.max(rows.length - 1, 0));
      if (reload && record?.request) void reopen(record.request);
    } catch (failure) {
      if (read === reads) error = message(failure);
    }
  }

  /** The query last searched; a changed query searches again from the newest page. */
  let searched: string | null = null;
  $effect(() => {
    const text = query;
    if (searched === null) {
      searched = text;
      return;
    }
    if (text === searched) return;
    const timer = setTimeout(() => void research(), 120);
    return () => clearTimeout(timer);
  });

  /** Search the query from the newest page now, unless it already was. */
  async function research() {
    if (query === searched) return;
    searched = query;
    start = 0;
    previous = [];
    index = 0;
    await load();
  }

  function older() {
    if (next === null) return;
    previous = [...previous, start];
    start = next;
    index = 0;
    void load();
  }

  function newer() {
    if (previous.length === 0) return;
    start = previous[previous.length - 1];
    previous = previous.slice(0, -1);
    index = 0;
    void load();
  }

  onMount(() => {
    void load();
    if (launch.request) void openId(launch.request);
    const clock = setInterval(() => void load(false), 1000);
    const stop = onStateChanged(() => void load());
    return () => {
      clearInterval(clock);
      stop();
    };
  });

  $effect(() => {
    // The read-only Preview of the selected request; opening it claims nothing. Every list
    // read (and clock redraw) shows it again from the same snapshot as the list.
    void rows;
    const row = selected;
    if (!row || focus !== "list") return;
    let current = true;
    const shown =
      row.kind === "human"
        ? call(commands.humanOpen(row.id, false)).then((human) => ({ human }))
        : call(commands.reviewDetail(row.id)).then((record) => ({ record }));
    shown.then(
      (view) => current && (preview = view),
      () => current && (preview = null),
    );
    return () => {
      current = false;
    };
  });

  /** Open a Human request; another request than the open one starts from a clean review. */
  function open(id: string, tools = false) {
    if (review?.id !== id) review = new Review(id, true);
    if (tools) review.area = "tools";
    record = null;
    focus = "detail";
    notice = null;
  }

  /** Read the read-only Detail of `id` again, keeping its toggles. */
  async function reopen(id: string) {
    try {
      const view = await call(commands.reviewDetail(id));
      if (record?.request === id) record = view;
    } catch (failure) {
      error = message(failure);
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
      review = null;
      if (record?.request !== view.request) {
        technical = false;
        pending = false;
        details = false;
      }
      record = view;
      focus = "detail";
      notice = null;
      void focusDetail();
    } catch (failure) {
      error = message(failure);
    }
  }

  /** Put focus in the open Detail, so its keys reach it (a reopened request keeps its Detail). */
  async function focusDetail() {
    await tick();
    const target = detailPane?.querySelector<HTMLElement>('[aria-label="Human review"], [role="document"]') ?? detailPane;
    if (target && !target.contains(document.activeElement)) target.focus();
  }

  async function back() {
    focus = "list";
    record = null;
    await tick();
    list?.focus();
    void load();
  }

  async function submitted(settled: Settled) {
    await load();
    // A result saved locally whose publication failed stays open with its failure, as in the
    // terminal review; it is never offered for submission again.
    if (settled === "savedLocally") return;
    review = null;
    await back();
    if (rows.length === 0) notice = waiting ? "No Human reviews are waiting." : "No request matches the search.";
  }

  async function search() {
    focus = "list";
    record = null;
    await tick();
    field?.focus();
    field?.select();
  }

  /** Leave the search for the list, which first shows what the query finds. */
  async function leave() {
    await research();
    await tick();
    list?.focus();
  }

  /** Screen-wide: Ctrl+O and Ctrl+F work wherever focus is while the Review screen shows. */
  function global(event: KeyboardEvent) {
    if (!active) return;
    if (chord(event, "o")) asking = true;
    else if (chord(event, "f")) void search();
    else return;
    event.preventDefault();
  }

  $effect(() => {
    // Showing the screen, or its first rows, puts focus on the list so its keys work at once;
    // focus already somewhere (the search field) stays there.
    const element = list;
    if (!active || focus !== "list" || !element) return;
    void tick().then(() => {
      const current = document.activeElement;
      if (!current || current === document.body || !current.closest(".review")) element.focus();
    });
  });

  function keydown(event: KeyboardEvent) {
    if (focus === "detail" && record) return recordKey(event);
    if (focus !== "list") return;
    if (plain(event, "ArrowDown") || plain(event, "j")) index = Math.min(index + 1, rows.length - 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) index = Math.max(index - 1, 0);
    else if ((plain(event, "Enter") || plain(event, "ArrowRight")) && selected) openRow(selected);
    else if (plain(event, "/")) void search();
    else if (plain(event, "PageDown")) older();
    else if (plain(event, "PageUp")) newer();
    else if (plain(event, "r")) void load();
    else return;
    event.preventDefault();
  }

  function recordKey(event: KeyboardEvent) {
    const agent = record?.profile === "agent";
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) {
      if (details) details = false;
      else void back();
    } else if (plain(event, "t") && (!agent || details)) technical = !technical;
    else if (plain(event, "w") && (!agent || details)) pending = !pending;
    else if (plain(event, "d") && agent) details = !details;
    else return;
    event.preventDefault();
  }

  $effect(() => {
    list?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  });

  useCommands(() =>
    active
      ? [
          { id: "review.open-id", group: "Review", label: "Open a request by ID", keys: "Ctrl+O", run: () => (asking = true) },
          { id: "review.search", group: "Review", label: "Search requests", keys: "/", run: () => void search() },
          {
            id: "review.waiting",
            group: "Review",
            label: "Show waiting Human reviews",
            enabled: () => query !== launch.search,
            run: () => (query = launch.search),
          },
          { id: "review.refresh", group: "Review", label: "Refresh the list", keys: "r", run: () => void load() },
          { id: "review.older", group: "Review", label: "Older requests", keys: "PgDn", enabled: () => next !== null, run: older },
          { id: "review.newer", group: "Review", label: "Newer requests", keys: "PgUp", enabled: () => previous.length > 0, run: newer },
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
<div class="review" onkeydown={keydown}>
  <nav class="crumbs" aria-label="Breadcrumb">
    <span class="root">artifactize review</span>
    <span class="sep">›</span>
    <span class="scope">{scope}</span>
    {#if focus === "detail" && review?.view}
      <span class="sep">›</span><span>{review.view.title.split(" · ")[0]}</span>
    {:else if focus === "detail" && record}
      <span class="sep">›</span><span>{record.title.split(" · ")[0]}</span>
    {/if}
    {#if error || notice}<span class="notice" class:error={!!error}>{error ?? notice}</span>{/if}
  </nav>
  <PaneGroup direction="horizontal" autoSaveId="review-columns">
    <Pane defaultSize={38} minSize={20}>
      <Panel
        title="{waiting ? 'Waiting Human reviews' : 'Requests'} ({rows.length}){page > 1 || next !== null ? ` · page ${page}` : ''}"
        focused={focus === "list"}
        onfocus={() => (focus = "list")}
      >
        {#snippet actions()}
          {#if page > 1 || next !== null}
            <button class="button small" disabled={previous.length === 0} onclick={newer} title="Newer requests (PgUp)">‹ Newer</button>
            <button class="button small" disabled={next === null} onclick={older} title="Older requests (PgDn)">Older ›</button>
          {/if}
          <button class="button small" onclick={() => (asking = true)}>Open ID <kbd>Ctrl+O</kbd></button>
        {/snippet}
        <SearchField bind:value={query} bind:input={field} complete={(text) => call(commands.reviewComplete(text))} onleave={() => void leave()} />
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
                  <ul role="listbox" aria-label="Requests" tabindex="0" bind:this={list}>
                    {#each rows as row, at (row.id)}
                      <li
                        role="option"
                        aria-selected={at === index}
                        class:selected={at === index}
                        data-index={at}
                        onclick={() => (at === index && focus === "list" ? openRow(row) : ((index = at), (focus = "list")))}
                        oncontextmenu={() => (index = at)}
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
        title={focus === "detail" ? (record ? `${record.title} · read-only` : "Review") : "Preview"}
        focused={focus === "detail"}
        bind:element={detailPane}
        onfocus={() => (review || record) && (focus = "detail")}
      >
        {#snippet actions()}
          {#if focus === "detail" && record}
            {#if record.profile === "agent"}
              <button class="button small" onclick={() => (details = !details)} aria-pressed={details}>Details <kbd>d</kbd></button>
            {/if}
            <button class="button small" onclick={() => void back()} title="Back to the list">Close <kbd>Esc</kbd></button>
          {/if}
        {/snippet}
        {#if focus === "detail" && record}
          {#key record.request}
            <RecordDetail detail={record} bind:technical bind:pending bind:details />
          {/key}
        {:else if focus === "detail" && review}
          {#key review}
            <HumanDetail {review} {active} onback={() => void back()} onsubmitted={(settled) => void submitted(settled)} />
          {/key}
        {:else if preview && "human" in preview && selected}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="preview" role="button" tabindex="-1" onclick={() => openRow(selected)}>
            <h3>{preview.human.title}</h3>
            <p class="meta">{preview.human.meta}</p>
            {#if preview.human.shared}<p class="shared">{preview.human.shared}</p>{/if}
            <p class="instruction">{preview.human.instruction}</p>
            <p>
              <strong>Tools</strong>
              {preview.human.tools.length === 0
                ? "none declared in this eval's scope"
                : preview.human.tools.map((tool) => `${tool.name} (${tool.kind})`).join(" · ")}
            </p>
            <p class="hint">Enter or click: open review</p>
          </div>
        {:else if preview && "record" in preview && selected}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="preview" role="button" tabindex="-1" onclick={() => openRow(selected)}>
            <h3>{preview.record.title}</h3>
            {#if preview.record.summary}<p class="meta {tone(preview.record.mark?.status)}">{preview.record.summary}</p>{/if}
            <Sections sections={preview.record.sections} peek />
            <p class="hint">Enter or click: open read-only</p>
          </div>
        {:else}
          <p class="empty">Select a request.</p>
        {/if}
      </Panel>
    </Pane>
  </PaneGroup>
</div>

<Dialog.Root bind:open={asking}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content
      class="dialog"
      onCloseAutoFocus={(event) => {
        // An opened request takes focus, not what had it before the dialog.
        if (focus !== "detail") return;
        event.preventDefault();
        void focusDetail();
      }}
    >
      <Dialog.Title class="dialog-title">Open a request by ID</Dialog.Title>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (wanted.trim()) {
            void openId(wanted.trim());
            asking = false;
            wanted = "";
          }
        }}
      >
        <input class="input" placeholder="req-…" bind:value={wanted} aria-label="Request ID" />
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
