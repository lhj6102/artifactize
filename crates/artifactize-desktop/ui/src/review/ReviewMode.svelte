<script lang="ts">
  // Review: its Pane lists the Human reviews waiting for a person, newest first; its main area
  // shows the chosen review, or one a reference opened (Open by ID, the launch's `review ID`),
  // as its evaluation space: with room, the Tool log as the view and the review's column at the
  // right. A request opened by ID that is not a waiting review shows read-only in the same place.
  // ↑ ↓ move through the list, Enter opens the selected review, Esc in a review steps back to
  // the list.
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import { call, message, type BrowseView, type WaitingRow } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Panel from "../shell/Panel.svelte";
  import Icon from "../shell/Icon.svelte";
  import { statusIcon } from "../shell/icons";
  import { scrollTo } from "../shell/attach";
  import { plain } from "../shell/keys";
  import RecordDetail from "../monitor/RecordDetail.svelte";
  import HumanDetail from "./HumanDetail.svelte";
  import type { ReviewFocus } from "./review-mode-state.svelte";

  const app = getAppState();
  const { browse, reviewMode: r } = app;

  const rows = $derived(browse.view?.waiting ?? []);
  const shown = $derived(r.shown);
  const review = $derived(shown.kind === "review" ? app.reviews[shown.id] : undefined);
  /** Its window is shown with Review: only then the review offers palette commands. */
  const active = $derived(app.mode === "review" && !app.monitor.settings);

  const title = $derived.by(() => {
    // A completed review is read-only, like any request that waits for nothing (D-REV-6).
    if (shown.kind === "review") return review?.settled ? "Review · read-only" : "Review";
    if (shown.kind === "request") return `${shown.detail.title} · read-only`;
    return "Review";
  });

  /** Open the review of `row` in the main area, the keyboard following it. */
  function open(row: WaitingRow) {
    app.showReview(row.id);
    void focus("main");
  }

  /** Move the list's selection by `by` rows. */
  function move(by: number) {
    if (rows.length === 0) return;
    const at = rows.findIndex((row) => row.id === r.selected);
    r.selected = rows[Math.max(0, Math.min(rows.length - 1, at < 0 ? 0 : at + by))].id;
  }

  function paneKey(event: KeyboardEvent) {
    if (plain(event, "ArrowDown") || plain(event, "j")) move(1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) move(-1);
    else if (plain(event, "Enter")) {
      const row = rows.find((item) => item.id === r.selected);
      if (row) open(row);
      else return;
    } else return;
    event.preventDefault();
  }

  /** A read-only request has its folds; Esc and ← go back to the list. A review takes its own
   * keys. */
  function mainKey(event: KeyboardEvent) {
    if (shown.kind !== "request") return;
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) void focus("pane");
    else if (r.folds.log) return;
    else if (plain(event, "t")) app.sections.toggle(shown.detail.target === "run" ? "Technical" : "Debug");
    else return;
    event.preventDefault();
  }

  /** Close of a request or a review returns to the list; a review first cancels its running
   * work, then leaves its form, as Esc in it does. */
  function close() {
    if (review === undefined || review.escape()) void focus("pane");
  }

  /** Move focus to the list or the main area; the Pane shows first when it is hidden. */
  async function focus(to: ReviewFocus) {
    if (to === "pane") app.paneShown = true;
    r.focus = to;
    await tick();
    const pane = r.panes[to];
    if (to === "main" && review) (pane?.querySelector<HTMLElement>('[aria-label="Human review"]') ?? pane)?.focus();
    else pane?.focus();
  }

  /** What a snapshot changes here, as it is accepted. */
  function accept(next: BrowseView) {
    // A replaced state: the reviews of the old one no longer apply.
    if (next.replaced && r.shown.kind !== "none") r.shown = { kind: "none" };
    // The state changed (another process, Ctrl+R or F5): a request shown by ID is read again.
    const byId = r.shown.kind === "request" ? r.shown.detail.request : null;
    if (byId !== null && next.revision !== r.followed) void reread(byId);
    r.followed = next.revision;
    start(next);
  }

  /** Read the request shown by ID again, unless another one was opened meanwhile. */
  async function reread(id: string) {
    try {
      const detail = await call(app.ipc.requestDetail(id));
      if (r.shown.kind === "request" && r.shown.detail.request === id) r.shown = { kind: "request", detail };
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** A review's launch, once, on the first snapshot read: that review, or the newest waiting. */
  function start(next: BrowseView) {
    if (r.started || !next.refreshed || !app.launch) return;
    r.started = true;
    if (app.launch.mode !== "review") return;
    app.mode = "review";
    if (app.launch.request) void app.openRequest(app.launch.request).then(() => focus("main"));
    else if (next.waiting.length > 0) open(next.waiting[0]);
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });
</script>

<div class="mode">
  <PaneGroup direction="horizontal" autoSaveId="review-layout" class="columns">
    {#if app.paneShown}
      <Split id="review-pane" order={1} defaultSize={24} minSize={12}>
        <section class="pane" class:focused={r.focus === "pane"} aria-label="Reviews" onfocusin={() => (r.focus = "pane")}>
          <!-- The keys move one selection through the list; they bubble up from any row. -->
          <ul
            class="list"
            role="listbox"
            aria-label="Human reviews"
            tabindex="0"
            bind:this={r.panes.pane}
            onkeydown={paneKey}
            {@attach scrollTo(() => (r.selected === null ? ":not(*)" : `[data-key="${CSS.escape(r.selected)}"]`))}
          >
            {#each rows as row (row.id)}
              <li role="option" aria-selected={r.selected === row.id} class:selected={r.selected === row.id} data-key={row.id} onclick={() => open(row)} onkeydown={() => {}}>
                <span class="glyph tone-human"><Icon name={statusIcon(row.mark.status)} /></span>
                <span class="eval">{row.eval}</span>
                <span class="sub">{[row.project, row.branch, row.age].filter((part) => part !== "").join(" · ")}{#if row.claim !== "-"}{` · ${row.claim}`}{/if}</span>
              </li>
            {:else}
              <li class="empty">{browse.view?.refreshed ? "No Human review waits for a person." : "Loading…"}</li>
            {/each}
          </ul>
        </section>
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="review-main" order={2} defaultSize={app.paneShown ? 76 : 100} minSize={30}>
      <Panel
        {title}
        label="Review's main area"
        focused={r.focus === "main"}
        notice={r.error}
        bind:noticeOpen={r.noticeOpen}
        bind:element={r.panes.main}
        onfocus={() => (r.focus = "main")}
        onkeydown={mainKey}
      >
        {#snippet headline()}
          {#if shown.kind === "review" && review?.view}
            <span class="title">{review.view.title}</span>
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if shown.kind !== "none" && !app.monitor.settings}
            <button class="button small" aria-label="Close (Esc)" aria-keyshortcuts="Escape" onclick={close}>Close</button>
          {/if}
        {/snippet}
        {#if shown.kind === "review"}
          {#if review}
            {#key review}
              <HumanDetail {review} {active} wide onback={() => void focus("pane")} />
            {/key}
          {/if}
        {:else if shown.kind === "request"}
          {#key shown.detail.request}
            <!-- A read-only request reads as a review does: one column at the left, no wider than the measure. -->
            <div class="record"><RecordDetail detail={shown.detail} bind:log={r.folds.log} /></div>
          {/key}
        {:else}
          <p class="empty">{app.paneShown ? "Choose a review in the Pane." : "Show the Pane with the Pane switch to choose a review."}</p>
        {/if}
      </Panel>
    </Split>
  </PaneGroup>
</div>

<style>
  .mode {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
    border-top: 2px solid transparent;
  }
  .pane.focused {
    border-top-color: var(--accent);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 4px 0;
    list-style: none;
    outline: none;
  }
  .list:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* A two-line row: the status icon and the eval, under it where it waits, for how long and by
   * whom it is claimed (D-REV-7). */
  .list li {
    display: grid;
    grid-template-columns: var(--icon) minmax(0, 1fr);
    gap: 0 8px;
    padding: 4px var(--pad);
    cursor: pointer;
  }
  .list li:hover {
    background: var(--surface-group);
  }
  .list li.selected {
    background: var(--selection);
  }
  .list:focus-within li.selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .glyph {
    display: inline-flex;
    align-items: center;
  }
  .eval,
  .sub {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    grid-column: 2;
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }
  .list li.empty {
    display: block;
    color: var(--ink-2);
    font-size: var(--text-small);
    cursor: default;
  }
  .list li.empty:hover {
    background: none;
  }
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  .record {
    height: 100%;
    min-height: 0;
    max-width: var(--measure);
  }
  /* A review's eval and title, said once, in the header row. */
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-2);
  }
</style>
