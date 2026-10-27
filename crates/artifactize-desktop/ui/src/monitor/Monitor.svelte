<script lang="ts">
  // The window's one screen: the Pane on the left and the Main area on the right, always with
  // the divider between them. A chip shows its Run or the branch's Current in the Main area, a
  // waiting review opens there as Review, and Settings take its place until left. A click on an
  // Artifact (or on the Run's headline) opens its Detail, floating over the graph's right side;
  // dragging its left edge sets its width, kept like the split ratio.
  import { Dialog } from "bits-ui";
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { BranchKey, BranchView, BrowseView, Chip, WaitingRow } from "../ipc/client";
  import { call, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { chipKey, entryKey, keyOf, sourceOf, type Entry, type Focus } from "./monitor-state.svelte";
  import { GAP, clampWidth, saveWidth } from "./detail-width";
  import Pane from "./Pane.svelte";
  import Panel from "../shell/Panel.svelte";
  import RunView from "./RunView.svelte";
  import CurrentView from "./CurrentView.svelte";
  import DetailPane from "./DetailPane.svelte";
  import RecordDetail from "./RecordDetail.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";

  const app = getAppState();
  const { browse, current, monitor: m } = app;
  const { panes } = m;

  const view = $derived(browse.view);
  const branches = $derived(view?.branches ?? []);
  const waiting = $derived(view?.waiting ?? []);
  const shown = $derived(m.shown);
  const run = $derived(shown.kind === "run" && view?.run?.id === shown.run ? view.run : null);
  const source = $derived(sourceOf(shown));
  /** The branch whose Run or Current is shown. */
  const branch = $derived(
    shown.kind === "run" || shown.kind === "current" ? (branches.find((item) => keyOf(item.key) === keyOf(shown.branch)) ?? null) : null,
  );
  const revision = $derived(shown.kind === "current" ? (current.view?.revision ?? 0) : browse.revision);
  const review = $derived(shown.kind === "review" ? app.reviews[shown.id] : undefined);
  /** The pane's search: the library's, overridden while typed until a pause searches it; a
   * failed search puts the library's back. */
  let query = $derived(browse.query);
  let typing: ReturnType<typeof setTimeout> | undefined;

  /** The entries the keys move through: each branch and, expanded, its timeline, then the
   * Waiting stack. */
  const entries = $derived<Entry[]>([
    ...branches.flatMap((item): Entry[] => [
      { kind: "branch", branch: item.key },
      ...(m.expanded.has(keyOf(item.key)) ? item.chips.map((chip): Entry => ({ kind: "chip", branch: item.key, chip: chipKey(chip) })) : []),
    ]),
    ...waiting.map((row): Entry => ({ kind: "waiting", id: row.id })),
  ]);
  const branchOf = (key: BranchKey) => branches.find((candidate) => keyOf(candidate.key) === keyOf(key));

  /** The chip of `item` shown in the Main area, by its index. */
  function filled(item: BranchView): number | null {
    if ((shown.kind !== "run" && shown.kind !== "current") || keyOf(shown.branch) !== keyOf(item.key)) return null;
    const at = item.chips.findIndex((chip) => (shown.kind === "run" ? chip.kind === "run" && chip.id === shown.run : chip.kind === "current"));
    return at < 0 ? null : at;
  }

  function closeDetailOf(next: string | null) {
    if (m.opened && keyOf(m.opened.source) !== next) m.opened = null;
  }

  /** Show `chip` of `item`, selected in its timeline: a Run its Run, the Current its Current,
   * scanned again. */
  function choose(item: BranchView, chip: Chip) {
    show(item, chip);
    m.selected = { kind: "chip", branch: item.key, chip: chipKey(chip) };
  }

  /** Show `chip` of `item` in the Main area; a find field over the graph shown before closes. */
  function show(item: BranchView, chip: Chip) {
    m.finding = false;
    if (chip.kind === "run") {
      m.shown = { kind: "run", branch: item.key, run: chip.id };
      closeDetailOf(keyOf({ kind: "run", run: chip.id }));
      current.clear();
      void browse.open(chip.id);
    } else showCurrent(item);
  }

  function showCurrent(item: BranchView) {
    m.finding = false;
    m.shown = { kind: "current", branch: item.key };
    closeDetailOf(keyOf({ kind: "current", branch: item.key }));
    void browse.close();
    void current.scan(item.key);
  }

  /** Select a branch: its newest entry, the Run in progress or else its Current, shows. */
  function select(item: BranchView) {
    const first = item.chips[0];
    if (first) show(item, first);
    m.selected = { kind: "branch", branch: item.key };
  }

  /** A click on a branch: it expands, and its newest entry shows. */
  function open(item: BranchView) {
    m.expanded.add(keyOf(item.key));
    select(item);
  }

  /** Expand a branch to its timeline, or fold it back. */
  function toggle(item: BranchView) {
    const key = keyOf(item.key);
    if (m.expanded.has(key)) m.expanded.delete(key);
    else m.expanded.add(key);
  }

  /** Open a waiting review (or a Human request opened by ID) in the Main area. */
  function openReview(id: string) {
    app.review(id);
    m.finding = false;
    m.shown = { kind: "review", id };
    m.opened = null;
    current.clear();
    void browse.close();
  }

  function openWaiting(row: WaitingRow) {
    m.selected = { kind: "waiting", id: row.id };
    openReview(row.id);
    void focus("main");
  }

  /** Open a request by ID in any repository: a Human request as Review, any other read-only. */
  async function openId(id: string) {
    try {
      const detail = await call(app.ipc.requestDetail(id));
      if (detail.profile === "human") return openReview(id);
      m.folds = { technical: false, pending: false, log: false, debug: false };
      m.shown = { kind: "request", detail };
      m.finding = false;
      m.opened = null;
      current.clear();
      void browse.close();
      void focus("main");
    } catch (failure) {
      browse.error = message(failure);
    }
  }

  /** Move the selection by `by` entries: a branch shows its newest entry, a timeline entry
   * shows itself, a review is only selected. Down past a timeline's oldest entry, older Runs
   * are asked for before the next branch is reached. */
  function move(by: number) {
    if (entries.length === 0) return;
    const selected = m.selected;
    const at = entries.findIndex((entry) => entryKey(entry) === entryKey(selected));
    if (by > 0 && selected?.kind === "chip") {
      const item = branchOf(selected.branch);
      if (item?.older && chipKey(item.chips[item.chips.length - 1]) === selected.chip) return void browse.older(item.key);
    }
    const next = entries[Math.max(0, Math.min(entries.length - 1, at < 0 ? 0 : at + by))];
    if (entryKey(next) === entryKey(selected)) return;
    if (next.kind === "waiting") m.selected = next;
    else {
      const item = branchOf(next.branch);
      const chip = next.kind === "chip" ? item?.chips.find((candidate) => chipKey(candidate) === next.chip) : undefined;
      if (item && chip) choose(item, chip);
      else if (item) select(item);
    }
  }

  /** → expands the selected branch, or enters its timeline when expanded; ← steps from a
   * timeline entry back to its branch, or folds an expanded branch. */
  function step(by: number) {
    const selected = m.selected;
    if (selected === null || selected.kind === "waiting") return;
    const item = branchOf(selected.branch);
    if (!item) return;
    const key = keyOf(item.key);
    if (by > 0 && selected.kind === "branch") {
      const first = item.chips[0];
      if (!m.expanded.has(key)) m.expanded.add(key);
      else if (first) choose(item, first);
    } else if (by < 0 && selected.kind === "chip") m.selected = { kind: "branch", branch: item.key };
    else if (by < 0) m.expanded.delete(key);
  }

  /** Move focus to the Pane's entries, the Main area or the Detail. */
  async function focus(to: Focus) {
    m.focus = to;
    await tick();
    const pane = panes[to];
    if (to === "main" && review) (pane?.querySelector<HTMLElement>('[aria-label="Human review"]') ?? pane)?.focus();
    else pane?.focus();
  }

  /** What a snapshot changes here, as it is accepted. */
  function accept(next: BrowseView) {
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (next.replaced) {
      m.opened = null;
      app.forgetReviews();
      if (m.shown.kind === "review" || m.shown.kind === "request") m.shown = { kind: "none" };
      if (m.focus === "detail") m.focus = "main";
    }
    // Leaving the shown Run (or losing it) closes its Detail; so does losing the node, which the
    // Detail finds as it reads its row.
    const opened = m.opened;
    if (opened?.source.kind === "run" && opened.source.run !== next.run?.id) m.opened = null;
    // A Run of the branch whose Current is shown ended: scan it again.
    for (const item of next.branches) {
      const now = item.chips.flatMap((chip) => (chip.kind === "run" && chip.running ? [chip.id] : []));
      const before = m.running[keyOf(item.key)];
      m.running[keyOf(item.key)] = now;
      const ended = before !== undefined && before.some((id) => !now.includes(id));
      if (ended && m.shown.kind === "current" && keyOf(m.shown.branch) === keyOf(item.key)) void current.scan(item.key);
    }
    // The state changed (another process, Ctrl+R or F5): a request shown by ID is read again.
    const byId = m.shown.kind === "request" ? m.shown.detail.request : null;
    if (byId !== null && next.revision !== m.followed) void reread(byId);
    m.followed = next.revision;
    start(next);
  }

  /** Read the request shown by ID again, unless another one was opened meanwhile. */
  async function reread(id: string) {
    try {
      const detail = await call(app.ipc.requestDetail(id));
      if (m.shown.kind === "request" && m.shown.detail.request === id) m.shown = { kind: "request", detail };
    } catch (failure) {
      browse.error = message(failure);
    }
  }

  /** Open what the launch asked for, once, on the first snapshot read. */
  function start(next: BrowseView) {
    if (m.started || !next.refreshed || !app.launch) return;
    m.started = true;
    const launched = next.launch === null ? undefined : next.branches.find((item) => keyOf(item.key) === keyOf(next.launch!));
    const mode = app.launch.mode;
    if (mode === "review") {
      if (app.launch.request) void openId(app.launch.request);
      else if (next.waiting.length > 0) openWaiting(next.waiting[0]);
      return;
    }
    // The launched branch opens expanded, so the entry it shows is in view.
    if (launched) m.expanded.add(keyOf(launched.key));
    if (launched && mode === "graph") {
      showCurrent(launched);
      m.selected = { kind: "branch", branch: launched.key };
    } else if (launched) select(launched);
    void focus("pane");
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });

  function setQuery(text: string) {
    query = text;
    clearTimeout(typing);
    typing = setTimeout(
      () =>
        void browse.search(text).then((searched) => {
          if (!searched) query = browse.query;
        }),
      120,
    );
  }

  /** The Review of a request, if one was opened: rendering only looks. */
  const reviewOf = (id: string): Review | undefined => app.reviews[id];
  /** Open the Review of a request when its Detail arrives, never while rendering; the open
   * Detail notes it, so leaving the Detail asks that review first. */
  function noteReview(id: string): void {
    app.review(id);
    if (m.opened && m.opened.request !== id) m.opened = { ...m.opened, request: id };
  }

  /** A click on an Artifact: its Detail, on the Artifact's own tab; what the graph's find
   * field found opens on its eval's tab. */
  function openNode(node: string, tab: string | null = null) {
    if (source) m.opened = { source, node, tab };
  }

  /** A click on the Run's headline: the Run's Detail. */
  function openRun() {
    if (source && run) m.opened = { source, node: run.node, tab: null };
  }

  /** Another tab of the open Detail; a Human review's tab notes its request when it arrives. */
  function showTab(tab: string | null) {
    if (m.opened && m.opened.tab !== tab) m.opened = { source: m.opened.source, node: m.opened.node, tab };
  }

  /** Close the Detail; focus that was in it goes back to the Main area. Leaving one step at a
   * time: a Human review shown in it first cancels its running work, then leaves its form, and
   * only then the Detail closes. */
  function closeDetail() {
    if (m.opened === null) return;
    const held = m.opened.request === undefined ? undefined : app.reviews[m.opened.request];
    if (held && !held.escape()) return;
    const inside = panes.detail?.contains(document.activeElement) ?? false;
    m.opened = null;
    if (inside) void focus("main");
  }

  /** Close of a request or a review in the Main area returns to the Pane; a review first
   * cancels its running work, then leaves its form, as Esc in it does. */
  function closeMain() {
    if (review === undefined || review.escape()) void focus("pane");
  }

  /** Esc in the Main area closes its Detail, else steps back to the Pane. */
  function back() {
    if (m.opened) closeDetail();
    else void focus("pane");
  }

  function mainKey(event: KeyboardEvent) {
    // A review takes its own keys; a read-only request has its folds.
    if (shown.kind === "review") return;
    if (shown.kind === "request" && !plain(event, "Escape") && !plain(event, "ArrowLeft")) {
      if (m.folds.log) return;
      if (plain(event, "t") && shown.detail.card) m.folds.debug = !m.folds.debug;
      else if (plain(event, "t")) m.folds.technical = !m.folds.technical;
      else if (plain(event, "w")) m.folds.pending = !m.folds.pending;
      else return;
    } else if (plain(event, "Escape") || (shown.kind === "request" && plain(event, "ArrowLeft"))) back();
    else return;
    event.preventDefault();
  }

  /** Dragging the Detail's left edge: the mouse moves it wherever it goes until the button is
   * released, as the pane dividers follow it. */
  function grab(event: MouseEvent) {
    if (event.button !== 0) return;
    const stage = (event.currentTarget as HTMLElement).closest(".stage");
    if (!stage) return;
    event.preventDefault();
    const drag = (moved: MouseEvent) => {
      const room = stage.getBoundingClientRect();
      m.detailWidth = clampWidth(room.right - GAP - moved.clientX, room.width);
    };
    const drop = () => {
      window.removeEventListener("mousemove", drag);
      window.removeEventListener("mouseup", drop);
      if (m.detailWidth !== null) saveWidth(m.detailWidth);
    };
    window.addEventListener("mousemove", drag);
    window.addEventListener("mouseup", drop);
  }

  const title = $derived.by(() => {
    if (shown.kind === "run") return `Run ${shown.run}`;
    if (shown.kind === "current") return branch ? `Current · ${branch.project}${branch.name ? `  ${branch.name}` : ""}` : "Current";
    if (shown.kind === "review") return "Review";
    if (shown.kind === "request") return `${shown.detail.title} · read-only`;
    return "Nothing chosen";
  });
  const notice = $derived(browse.error ?? view?.notice ?? null);

  useCommands(() => [
    { id: "pane.search", group: "Pane", label: "Search branches and reviews", keys: "/", run: () => panes.search?.focus() },
    { id: "pane.focus", group: "Pane", label: "Focus the Pane", run: () => void focus("pane") },
    { id: "main.focus", group: "Main area", label: "Focus the Main area", run: () => void focus("main") },
    { id: "main.open-id", group: "Main area", label: "Open a request by ID", keys: "Ctrl+O", run: () => (app.asking = true) },
    { id: "main.run", group: "Main area", label: "Open the Run's Detail", enabled: () => run !== null, run: openRun },
    {
      id: "main.close",
      group: "Main area",
      label: "Close the Detail",
      keys: "Esc",
      enabled: () => m.opened !== null,
      run: closeDetail,
    },
  ]);
</script>

<div class="monitor">
  <PaneGroup direction="horizontal" autoSaveId="monitor-layout" class="columns">
    <Split defaultSize={24} minSize={12}>
      <Pane
        bind:query={() => query, setQuery}
        {filled}
        onselect={open}
        ontoggle={toggle}
        onchoose={choose}
        onopen={openWaiting}
        onmove={move}
        onstep={step}
        onmain={() => void focus("main")}
      />
    </Split>
    <PaneResizer class="resizer" />
    <Split defaultSize={76} minSize={30}>
      <div class="stage">
        <Panel
          {title}
          label="Main area"
          focused={m.focus === "main"}
          {notice}
          bind:noticeOpen={m.noticeOpen}
          bind:element={panes.main}
          onfocus={() => m.focus !== "detail" && (m.focus = "main")}
          onkeydown={mainKey}
        >
          {#snippet actions()}
            {#if (shown.kind === "request" || shown.kind === "review") && !m.settings}
              <button class="button small" onclick={closeMain}>Close <kbd>Esc</kbd></button>
            {/if}
          {/snippet}
          {#if shown.kind === "run"}
            {#if run}
              <RunView {run} {revision} opened={m.opened?.node ?? null} onopen={openNode} onclose={closeDetail} onrun={openRun} />
            {:else if view?.missing}
              <p class="empty">This Run cannot be read; choose another.</p>
            {:else}
              <p class="empty">Loading the Run…</p>
            {/if}
          {:else if shown.kind === "current"}
            <CurrentView branch={shown.branch} opened={m.opened?.node ?? null} onopen={openNode} onclose={closeDetail} />
          {:else if shown.kind === "review"}
            {#if review}
              <div class="review">
                {#if review.view}<p class="crumb">{review.view.title} · Artifact {review.view.artifact}</p>{/if}
                {#key review}
                  <HumanDetail {review} active={!m.settings} onback={() => void focus("pane")} />
                {/key}
              </div>
            {/if}
          {:else if shown.kind === "request"}
            {#key shown.detail.request}
              <RecordDetail detail={shown.detail} bind:technical={m.folds.technical} bind:pending={m.folds.pending} bind:log={m.folds.log} bind:debug={m.folds.debug} />
            {/key}
          {:else}
            <p class="empty">Choose a branch or a waiting review in the Pane.</p>
          {/if}
        </Panel>
        {#if m.opened && source && keyOf(m.opened.source) === keyOf(source)}
          <aside class="dock" aria-label="Detail" style:--detail-width={m.detailWidth === null ? null : `${m.detailWidth}px`}>
            <!-- The left edge takes a mouse drag, as the pane dividers do; hovering it shows only
                 the resize cursor. -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div class="edge" role="separator" aria-orientation="vertical" aria-label="Detail width" onmousedown={grab}></div>
            <DetailPane
              source={m.opened.source}
              {revision}
              {branch}
              node={m.opened.node}
              tab={m.opened.tab}
              focused={m.focus === "detail"}
              active={!m.settings}
              review={reviewOf}
              openReview={noteReview}
              bind:element={panes.detail}
              onfocus={() => (m.focus = "detail")}
              ontab={showTab}
              onopen={openNode}
              onback={closeDetail}
              ongone={() => (m.opened = null)}
            />
          </aside>
        {/if}
      </div>
    </Split>
  </PaneGroup>
</div>

<Dialog.Root bind:open={app.asking}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content
      class="dialog"
      onCloseAutoFocus={(event) => {
        // An opened request takes focus, not what had it before the dialog.
        if (m.focus !== "main") return;
        event.preventDefault();
        void focus("main");
      }}
    >
      <Dialog.Title class="dialog-title">Open a request by ID</Dialog.Title>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (m.wanted.trim()) {
            void openId(m.wanted.trim());
            app.asking = false;
            m.wanted = "";
          }
        }}
      >
        <input class="input" placeholder="req-…" bind:value={m.wanted} aria-label="Request ID" />
        <div class="dialog-actions">
          <Dialog.Close class="button">Cancel</Dialog.Close>
          <button class="button primary" type="submit">Open</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .monitor {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  :global(.columns) {
    min-height: 0;
  }
  /* The graph's own layers (canvas, Detail) stack inside the stage, so none of them covers the
   * divider's grab zone on the graph's side, and the Detail never sits beside the Pane. */
  .stage {
    position: relative;
    isolation: isolate;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* Docked toward the graph's right edge with a gap; the graph stays visible behind it. The
   * Detail sets its own width, by default or by a drag, within the graph's area (as
   * detail-width.ts fits a drag); its content never sizes it, and wraps or scrolls in its own
   * boxes. */
  .dock {
    position: absolute;
    z-index: 20;
    top: 12px;
    right: 12px;
    bottom: 12px;
    width: var(--detail-width, min(46%, 680px));
    min-width: min(320px, calc(100% - 24px));
    max-width: calc(100% - 108px);
    contain: inline-size;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgb(0 0 0 / 22%);
  }
  .edge {
    position: absolute;
    z-index: 1;
    top: 0;
    bottom: 0;
    left: 0;
    width: 6px;
    cursor: col-resize;
    touch-action: none;
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .review {
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-height: 0;
  }
  .crumb {
    margin: 0;
    padding: 0 var(--pad) 6px;
    color: var(--muted);
    overflow-wrap: anywhere;
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
