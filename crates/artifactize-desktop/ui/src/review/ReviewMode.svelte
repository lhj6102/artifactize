<script lang="ts">
  // Review: its Pane holds the filter (starting `type:human waiting`) and the reviews the library
  // finds for it, grouped by project, newest first; its main area shows the chosen review, or one
  // a reference opened (Open in Review, Open by ID, the launch's `review ID`), as its evaluation
  // space, its way back to the Run and Artifact first in the header row. A review the filter does
  // not list says why under that row. A request opened by ID that is not a Human review shows
  // read-only in the same place. ↑ ↓ move through the list, Enter opens the selected review, Esc
  // in a review steps back to the list.
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import { REVIEW_DEFAULT_QUERY, call, message, onStateChanged, type BrowseView, type ReviewMatch } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Panel from "../shell/Panel.svelte";
  import Icon from "../shell/Icon.svelte";
  import SearchField from "../shell/SearchField.svelte";
  import Reference from "../shell/Reference.svelte";
  import { statusIcon } from "../shell/icons";
  import { scrollTo } from "../shell/attach";
  import { plain } from "../shell/keys";
  import RecordDetail from "../monitor/RecordDetail.svelte";
  import HumanDetail from "./HumanDetail.svelte";
  import type { ReviewFocus } from "./review-mode-state.svelte";

  const app = getAppState();
  const { browse, reviewMode: r } = app;

  const rows = $derived(r.matches);
  /** The reviews grouped by project, each group in the list's order. */
  const groups = $derived.by(() => {
    const byProject: [string, ReviewMatch[]][] = [];
    for (const row of rows) {
      const group = byProject.find(([project]) => project === row.project);
      if (group) group[1].push(row);
      else byProject.push([row.project, [row]]);
    }
    return byProject;
  });
  const shown = $derived(r.shown);
  const review = $derived(shown.kind === "review" ? app.reviews[shown.id] : undefined);
  /** Its window is shown with Review: only then the review offers palette commands. */
  const active = $derived(app.mode === "review" && !app.monitor.settings);
  /** The filter as typed: the searched one, overridden while typed until a pause searches it. */
  let query = $derived(r.query);
  let typing: ReturnType<typeof setTimeout> | undefined;

  const title = $derived.by(() => {
    if (shown.kind === "review") return review?.view ? review.view.title.split(" · ")[0] : "Review";
    if (shown.kind === "request") return `${shown.detail.title} · read-only`;
    return "Review";
  });
  /** The review's title, after its eval: said once, in the header row. */
  const subtitle = $derived(shown.kind === "review" && review?.view ? review.view.title.split(" · ").slice(1).join(" · ") : "");

  /** Search the library for `text`; the newest search wins. */
  let searches = 0;
  async function search(text: string): Promise<void> {
    const mine = ++searches;
    try {
      const page = await call(app.ipc.humanSearch(text, null));
      if (mine !== searches) return;
      r.query = text;
      r.matches = page.reviews;
      r.next = page.next;
      r.searched = true;
      r.error = null;
      // The shown review, listed again, is no longer outside the filter.
      if (r.outside !== null && shown.kind === "review" && page.reviews.some((row) => row.id === shown.id)) r.outside = null;
    } catch (failure) {
      if (mine === searches) r.error = message(failure);
    }
  }

  function setQuery(text: string) {
    query = text;
    clearTimeout(typing);
    typing = setTimeout(() => void search(text), 120);
  }

  /** Open the review of `row` in the main area, the keyboard following it. */
  function open(row: ReviewMatch) {
    void app.showReview(row.id).then(() => focus("main"));
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

  /** The way back: the review's Run in Verify, its Artifact's Detail open on the review's eval. */
  function back() {
    const target = review?.view?.target;
    if (!target) return;
    const eval_ = target.node;
    const artifact = eval_.startsWith("e:") ? `a:${eval_.slice(2).split("/")[0]}` : eval_;
    app.showRun(target.run, artifact, eval_);
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
    else void search(r.query).then(() => {
      if (r.matches.length > 0) open(r.matches[0]);
    });
  }

  onMount(() => {
    void search(r.query);
    if (browse.view) accept(browse.view);
    const unshown = browse.onShown(accept);
    // A claim, a submission or a new review elsewhere: the list follows it.
    const unchanged = onStateChanged(() => void search(r.query));
    return () => {
      unshown();
      unchanged();
    };
  });
</script>

<div class="mode">
  <PaneGroup direction="horizontal" autoSaveId="review-layout" class="columns">
    {#if app.paneShown}
      <Split id="review-pane" order={1} defaultSize={24} minSize={12}>
        <section class="pane" class:focused={r.focus === "pane"} aria-label="Reviews" onfocusin={() => (r.focus = "pane")}>
          <SearchField
            bind:value={() => query, setQuery}
            bind:input={r.panes.filter}
            label="Filter the reviews"
            placeholder="Filter: text, type:human, waiting, repo:, branch:"
            reset={REVIEW_DEFAULT_QUERY}
            complete={(text) => call(app.ipc.humanSearchComplete(text))}
            onleave={() => r.panes.pane?.focus()}
          />
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
            {#each groups as [project, members] (project)}
              <li class="group" role="presentation">{project}</li>
              {#each members as row (row.id)}
                <li role="option" aria-selected={r.selected === row.id} class:selected={r.selected === row.id} data-key={row.id} onclick={() => open(row)} onkeydown={() => {}}>
                  <span class="glyph tone-human"><Icon name={statusIcon(row.mark.status)} /></span>
                  <span class="eval">{row.eval}</span>
                  <span class="age">{row.claimant ? `${row.claimant} · ` : ""}{row.age}</span>
                </li>
              {/each}
            {:else}
              <li class="empty">{r.searched ? "No review matches the filter." : "Loading…"}</li>
            {/each}
          </ul>
        </section>
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="review-main" order={2} defaultSize={app.paneShown ? 76 : 100} minSize={30}>
      <div class="main">
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
          {#snippet lead()}
            {#if shown.kind === "review" && review?.view}
              <!-- The way back, first: the Run that asked for the review, and its Artifact. -->
              <Reference chip onclick={back}>Verify · <span class="mono">{review.view.target.run}</span> › {review.view.artifact}</Reference>
            {/if}
          {/snippet}
          {#snippet headline()}
            {#if subtitle}<span class="title">{subtitle}</span>{/if}
          {/snippet}
          {#snippet actions()}
            {#if shown.kind !== "none" && !app.monitor.settings}
              <button class="button small" aria-label="Close (Esc)" aria-keyshortcuts="Escape" onclick={close}>Close</button>
            {/if}
          {/snippet}
          <div class="body" class:outside={r.outside !== null}>
            {#if r.outside !== null && shown.kind === "review"}
              <p class="outside-line" role="status"><Icon name="open" />Opened from a reference · outside the filter ({r.outside})</p>
            {/if}
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
              <p class="empty">{app.paneShown ? "Choose a review in the Pane." : "Show the Pane from the command palette to choose a review."}</p>
            {/if}
          </div>
        </Panel>
      </div>
    </Split>
  </PaneGroup>
</div>

<style>
  .mode,
  .main {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* The review's place under the header row, with the dock's width its column takes. */
  .main {
    --dock-width: clamp(min(320px, 100%), var(--detail-width, min(46%, 680px)), calc(100% - 240px));
  }
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 0 0 4px;
    list-style: none;
    outline: none;
  }
  .list:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* A project's name over its reviews: a group label (D-SYS-5). */
  .group {
    padding: 10px var(--pad) 4px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  /* One line per review: the status icon, the eval, and how long it waits at the right
   * (D-REV-7). */
  .list li[role="option"] {
    display: grid;
    grid-template-columns: var(--icon) minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    min-height: var(--row);
    padding: 0 var(--pad);
    cursor: pointer;
  }
  .list li[role="option"]:hover {
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
  .eval {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .age {
    color: var(--ink-3);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .list li.empty {
    padding: 6px var(--pad);
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  .body {
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .body.outside {
    grid-template-rows: auto minmax(0, 1fr);
  }
  /* Under the header row: a review opened from a reference outside the filter (D-REV-8). */
  .outside-line {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    min-height: var(--row-dense);
    padding: 0 var(--pad);
    border-top: 1px solid var(--line-group);
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .record {
    height: 100%;
    min-height: 0;
    max-width: var(--measure);
  }
  .mono {
    font-family: var(--mono);
  }
  /* A review's title, said once, in the header row. */
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-2);
  }
</style>
