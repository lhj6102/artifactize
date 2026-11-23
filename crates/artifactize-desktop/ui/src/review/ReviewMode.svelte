<script lang="ts">
  // Review: its Pane holds the filter (starting `type:human waiting`) and the reviews the library
  // finds for it, grouped by project, newest first; its main area shows the chosen review, or one
  // a reference opened (Open in Review, Open by ID, the launch's `review ID`), as its evaluation
  // space, its way back to the Run and Artifact first in the header row. A review the filter does
  // not list says why under that row. A request opened by ID that is not a Human review shows
  // read-only in the same place. ↑ ↓ move through the list, Enter opens the selected review, Esc
  // in a review steps back to the list.
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick, untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import type { Attachment } from "svelte/attachments";
  import { Coalesced } from "../ipc/coalesced";
  import { REVIEW_DEFAULT_QUERY, call, CommandError, message, onStateChanged, type BrowseView, type ReviewMatch } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Stage from "../shell/Stage.svelte";
  import Icon from "../shell/Icon.svelte";
  import SearchField from "../shell/SearchField.svelte";
  import Reference from "../shell/Reference.svelte";
  import { tone } from "../shell/tone";
  import { statusIcon } from "../shell/icons";
  import { scrollTo } from "../shell/attach";
  import { composing, plain } from "../shell/keys";
  import { mark } from "../shell/find";
  import RecordDetail from "../monitor/RecordDetail.svelte";
  import HumanDetail from "./HumanDetail.svelte";
  import type { ReviewFocus } from "./review-mode-state.svelte";

  const app = getAppState();
  const { browse, reviewMode: r } = app;

  const rows = $derived(r.invalidFilter ? [] : r.matches);
  /** Group by project within each page; an appended page never rearranges existing rows. */
  const groups = $derived.by(() => {
    const groups: { key: string; project: string; rows: ReviewMatch[] }[] = [];
    let start = 0;
    const ends = r.pageEnds.length ? r.pageEnds : [rows.length];
    for (const [page, end] of ends.entries()) {
      const next: { key: string; project: string; rows: ReviewMatch[] }[] = [];
      for (const row of rows.slice(start, end)) {
        const key = `${page}:${row.projectKey}`;
        const group = next.find((each) => each.key === key);
        if (group) group.rows.push(row);
        else next.push({ key, project: row.projectKey, rows: [row] });
      }
      const last = groups.at(-1);
      if (last && next[0]?.project === last.project) last.rows.push(...next.shift()!.rows);
      groups.push(...next);
      start = end;
    }
    return groups;
  });
  const shown = $derived(r.shown);
  const review = $derived(shown.kind === "review" ? app.reviews[shown.id] : undefined);
  /** Its window is shown with Review: only then the review offers palette commands. */
  const active = $derived(app.mode === "review" && !app.monitor.settings);
  /** The filter as typed: the searched one, overridden while typed until a pause searches it. */
  let query = $derived(r.invalidFilter ?? r.query);
  let typing: ReturnType<typeof setTimeout> | undefined;

  const title = $derived.by(() => {
    if (shown.kind === "review") return review?.view ? review.view.title.split(" · ")[0] : "Review";
    if (shown.kind === "request") return `${shown.detail.title} · read-only`;
    return "Review";
  });
  /** The review's title, after its eval: said once, in the header row. */
  const subtitle = $derived(shown.kind === "review" && review?.view ? review.view.title.split(" · ").slice(1).join(" · ") : "");

  /** The column's head: the eval's title; led by the eval itself when no row of the Pane names
   * the review (opened by a reference the filter does not list). */
  const headTitle = $derived.by(() => {
    if (shown.kind !== "review" || !review?.view) return null;
    const listed = rows.some((row) => row.id === shown.id);
    return listed ? subtitle || title : review.view.title;
  });

  /** Find in the conversation (J-REVIEW-BACK): what is looked for and which place is shown. */
  let find = $state<{ text: string; at: number } | null>(null);
  let found = $state(0);
  let lastShown = "";
  const shownPlace = $derived(found === 0 || !find ? 0 : ((find.at % found) + found) % found);
  /** Mark what Find looks for in the shown conversation; re-marks as it changes. */
  const marking: Attachment<HTMLElement> = (body) => {
    const looking = find;
    void review?.view;
    void review?.draft;
    void review?.log;
    if (!looking) {
      found = 0;
      lastShown = "";
      return;
    }
    const marked = mark(body, looking.text, looking.at, lastShown);
    found = marked.found;
    lastShown = marked.shown;
    return marked.clear;
  };
  /** Enter shows the next place, Shift+Enter the one before; Esc closes the field. */
  function findKey(event: KeyboardEvent) {
    const looking = find;
    if (!looking || composing(event)) return;
    if (event.key === "Enter") {
      event.preventDefault();
      find = { text: looking.text, at: looking.at + (event.shiftKey ? -1 : 1) };
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      find = null;
      void focus("main");
    }
  }

  let typingRevision = 0;
  let pending: { text: string } | null = null;
  let openNewest = false;
  let loadingMore = $state(false);

  /** All search IPC, including typing, refreshes and the next cursor, goes through one reader. */
  async function read(): Promise<void> {
    if (typing !== undefined) return;
    const typed = pending;
    pending = null;
    if (!typed && r.invalidFilter !== null) return;
    const revision = typingRevision;
    const text = typed?.text ?? r.query;
    const more = !typed && loadingMore;
    try {
      let cursor = more ? r.next : null;
      if (more && cursor === null) return;
      const rows = new SvelteMap((more ? r.matches : []).map((row) => [row.id, row]));
      const ends = more ? [...r.pageEnds] : [];
      const wanted = r.pageEnds.length || 1;
      do {
        const page = await call(app.ipc.humanSearch(text, cursor));
        if (revision !== typingRevision) return;
        for (const row of page.reviews) rows.set(row.id, row);
        ends.push(rows.size);
        cursor = page.next === cursor ? null : page.next;
      } while (!more && !typed && cursor !== null && ends.length < wanted);
      const top = more ? r.panes.pane?.scrollTop : undefined;
      r.query = text;
      r.invalidFilter = null;
      r.matches = [...rows.values()];
      r.pageEnds = ends;
      r.next = cursor;
      r.searched = true;
      r.error = null;
      if (openNewest) {
        openNewest = false;
        if (r.matches[0]) open(r.matches[0]);
      }
      if (top !== undefined) {
        await tick();
        if (r.panes.pane) r.panes.pane.scrollTop = top;
      }
    } catch (failure) {
      if (revision !== typingRevision) return;
      if (typed) {
        if (failure instanceof CommandError && failure.failure.kind === "invalid") r.invalidFilter = text;
        else { r.invalidFilter = null; query = r.query; }
      }
      r.error = message(failure);
    } finally {
      if (more) loadingMore = false;
    }
  }

  const refresh = new Coalesced(read, () => app.mode === "review" || !r.searched);

  function setQuery(text: string) {
    typingRevision++;
    loadingMore = false;
    pending = null;
    r.invalidFilter = null;
    query = text;
    clearTimeout(typing);
    typing = setTimeout(() => {
      typing = undefined;
      pending = { text };
      refresh.request();
    }, 120);
  }

  function moreReviews() {
    if (r.next === null || loadingMore || typing !== undefined || pending || r.invalidFilter !== null) return;
    loadingMore = true;
    refresh.request();
  }

  /** Open the review of `row` in the main area, the keyboard following it. */
  function open(row: ReviewMatch) {
    void app.showReview(row.id).then(() => focus("main"));
  }

  /** Move the keyboard cursor without changing which review is open. */
  function move(by: number) {
    if (rows.length === 0) return;
    const at = rows.findIndex((row) => row.id === r.selected);
    r.selected = rows[Math.max(0, Math.min(rows.length - 1, at < 0 ? 0 : at + by))].id;
  }

  function paneKey(event: KeyboardEvent) {
    if (event.target instanceof HTMLButtonElement) return;
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

  /** The way back and Open in Verify: the review's Run in Verify, its Artifact's Detail open on
   * the review's eval. */
  function goBack() {
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
    else {
      openNewest = true;
      refresh.request();
    }
  }

  /** A claim, a submission or a new review elsewhere: the list follows it, one search at a time
   * and only while Review is on screen; a change missed while it was not is read when it is. */
  const following: Attachment = () => {
    if (app.mode === "review") untrack(() => refresh.showing());
  };

  onMount(() => {
    if (browse.view) accept(browse.view);
    if (!openNewest && !r.searched) refresh.request();
    const unshown = browse.onShown(accept);
    const unchanged = onStateChanged(() => refresh.request());
    return () => {
      clearTimeout(typing);
      unshown();
      unchanged();
    };
  });
</script>

<div class="mode" {@attach following}>
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
            {#each groups as group (group.key)}
              <li class="group" role="presentation">{group.rows[0]?.project}</li>
              {#each group.rows as row (row.id)}
                {@const opened = r.shown.kind === "review" && r.shown.id === (app.reviews[row.id]?.id ?? row.id)}
                <li role="option" aria-selected={opened} class:selected={opened} class:cursor={r.selected === row.id} data-key={row.id} onclick={() => open(row)} onkeydown={() => {}}>
                  <span class="glyph {tone(row.mark.status)}"><Icon name={statusIcon(row.mark.status)} label={row.mark.status.replaceAll("_", " ")} /></span>
                  <span class="eval">{row.eval}</span>
                  <span class="age">{#if row.claimant}<span class="claimant">{row.claimant}</span> · {/if}{row.age}</span>
                </li>
              {/each}
            {:else}
              <li class="empty">{r.searched ? "No review matches the filter." : "Loading…"}</li>
            {/each}
            {#if r.next !== null && !r.invalidFilter}
              <li role="presentation"><button class="more" aria-disabled={loadingMore} onclick={moreReviews}>More reviews</button></li>
            {/if}
          </ul>
        </section>
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="review-main" order={2} defaultSize={app.paneShown ? 76 : 100} minSize={30}>
      <div class="main" style:--detail-width={app.detailWidth === null ? null : `${app.detailWidth}px`}>
        <!-- The header row holds controls only (D-REV-8): the way back, and Close; the Pane's
             selected row names the review's eval, the column's head its title. -->
        <Stage
          label="Review's main area"
          notice={r.error}
          bind:noticeOpen={r.noticeOpen}
          bind:element={r.panes.main}
          onfocus={() => (r.focus = "main")}
          onkeydown={mainKey}
        >
          {#snippet back()}
            {#if shown.kind === "review" && review?.view && r.back}
              <!-- The way back, only for a review opened from Verify: its Run and Artifact. -->
              <Reference back onclick={goBack}>Verify · <span class="mono">{review.view.target.run}</span> › {review.view.artifact}</Reference>
            {/if}
          {/snippet}
          {#snippet tools()}
            {#if shown.kind === "review" && review?.view && !app.monitor.settings}
              {#if find}
                {@const looking = find}
                <label class="find">
                  <Icon name="find" />
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    aria-label="Find in the conversation"
                    value={looking.text}
                    autofocus
                    oninput={(event) => (find = { text: event.currentTarget.value, at: 0 })}
                    onblur={(event) => { if (event.currentTarget.value === "") find = null; }}
                    onkeydown={findKey}
                  />
                  {#if looking.text.trim() !== ""}<span class="count" role="status">{found === 0 ? "No match" : `${shownPlace + 1} of ${found}`}</span>{/if}
                </label>
              {:else}
                <button class="tool" aria-label="Find in the conversation" title="Find in the conversation" onclick={() => (find = { text: "", at: 0 })}><Icon name="find" /></button>
              {/if}
              <span class="tool-sep"></span>
              <button class="tool" onclick={goBack}><Icon name="open" />Open in Verify</button>
            {/if}
          {/snippet}
          <div class="body" {@attach marking}>
            {#if shown.kind === "review"}
              {#if review}
                {#key review}
                  <HumanDetail {review} {active} wide title={headTitle} onback={() => void focus("pane")} />
                {/key}
              {/if}
            {:else if shown.kind === "request"}
              {#key shown.detail.request}
                <!-- A read-only request reads as a review does: one column at the left, no wider than the measure. -->
                <div class="record">
                  <!-- Not in the Pane's list: the column's head names it, read-only (D-REV-6). -->
                  <p class="record-title">{title}</p>
                  <RecordDetail detail={shown.detail} bind:log={r.folds.log} />
                  <!-- The decision bar says why nothing is written here (D-REV-6, D-REV-12). -->
                  <div class="readonly" role="group" aria-label="Decision">
                    <span>{shown.detail.profile === "human" ? "This review no longer waits for a person: read-only." : "Only a waiting Human review is written here: read-only."}</span>
                  </div>
                </div>
              {/key}
            {:else}
              <p class="empty">{app.paneShown ? "Choose a review in the Pane." : "Show the Pane from the command palette to choose a review."}</p>
            {/if}
          </div>
        </Stage>
      </div>
    </Split>
  </PaneGroup>
</div>

<style>
  /* Find in the conversation: the find tool's field in its place, as Chat's (D-CHAT-11). */
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
    outline: 1px solid var(--current-mark);
    outline-offset: -2px;
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
  .list:focus-visible:not(:has(li.cursor)),
  .list:focus-visible li.cursor {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* A project's name over its reviews: a group label (D-SYS-5). */
  .group {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    grid-template-columns: var(--icon) minmax(0, 1fr) fit-content(45%);
    align-items: center;
    gap: 8px;
    height: var(--row-pane);
    padding: 0 var(--pad);
    color: var(--ink-2);
    cursor: pointer;
  }
  .list li[role="option"]:not(.selected):hover {
    background: var(--surface-group);
  }
  .list li.selected {
    background: var(--selection);
    color: var(--ink);
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
    display: flex;
    gap: 3px;
    min-width: 0;
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .claimant {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    width: 100%;
    height: var(--row-pane);
    padding: 0 var(--pad);
    border: 0;
    background: none;
    text-align: left;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .more:hover { background: var(--surface-group); }
  .more:focus-visible { outline-offset: -2px; }
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
  .record {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    max-width: var(--measure);
    margin: 0 auto;
  }
  .readonly {
    flex: none;
    display: flex;
    align-items: center;
    min-height: 40px;
    margin: 8px var(--pad) 12px;
    padding: 4px 8px;
    box-sizing: border-box;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    background: var(--surface-panel);
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .record-title {
    flex: none;
    margin: 0;
    padding: 12px var(--pad) 6px;
    color: var(--ink-bright);
    font-size: var(--text-lead);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
