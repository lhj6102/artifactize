<script lang="ts">
  // Verify: the Pane on the left and the Main area on the right, with the divider between them
  // while the Pane is shown; the bar's Pane switch (Ctrl+B) hides both, the Main area taking the
  // workspace's width, and shows them again at the width the Pane had. A chip shows its Run or
  // the branch's Current in the Main area, under one header row with its headline. A click on an
  // Artifact (or on the Run's headline) opens its Detail, a panel docked at the graph's right
  // edge under its tool strip; dragging its left edge sets its width, kept like the split ratio.
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BranchKey, BranchView, BrowseView, Chip } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { chipKey, entryKey, keyOf, sourceOf, type Entry, type Focus } from "./monitor-state.svelte";
  import { KEEP, MIN, clampWidth, saveWidth } from "./detail-width";
  import Pane from "./Pane.svelte";
  import Panel from "../shell/Panel.svelte";
  import RunView from "./RunView.svelte";
  import GraphTools from "./GraphTools.svelte";
  import RunHeadline from "./RunHeadline.svelte";
  import CurrentView from "./CurrentView.svelte";
  import ScanState from "./ScanState.svelte";
  import DetailPane from "./DetailPane.svelte";
  import type { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";
  import EditDialogs from "./EditDialogs.svelte";
  import Reference from "../shell/Reference.svelte";

  const app = getAppState();
  const { browse, current, definitions, monitor: m } = app;
  const { panes } = m;

  const view = $derived(browse.view);
  const branches = $derived(view?.branches ?? []);
  const shown = $derived(m.shown);
  const run = $derived(shown.kind === "run" && view?.run.kind === "read" && view.run.id === shown.run ? view.run : null);
  const source = $derived(sourceOf(shown));
  /** The branch whose Run or Current is shown, also while the Pane's search leaves it out. */
  const branch = $derived(shown.kind === "run" || shown.kind === "current" ? browse.branch(shown.branch) : null);
  const revision = $derived(shown.kind === "current" ? (current.view?.revision ?? 0) : browse.revision);
  /** The pane's search: the library's, overridden while typed until a pause searches it; a
   * failed search puts the library's back. */
  let query = $derived(browse.query);
  let typing: ReturnType<typeof setTimeout> | undefined;

  /** The entries the keys move through: each branch and, expanded, its timeline. */
  const entries = $derived<Entry[]>(
    branches.flatMap((item): Entry[] => [
      { kind: "branch", branch: item.key },
      ...(m.expanded.has(keyOf(item.key)) ? item.chips.map((chip): Entry => ({ kind: "chip", branch: item.key, chip: chipKey(chip) })) : []),
    ]),
  );
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

  /** Move the selection by `by` entries: a branch shows its newest entry, a timeline entry
   * shows itself. Down past a timeline's oldest entry, older Runs
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
    const item = branchOf(next.branch);
    const chip = next.kind === "chip" ? item?.chips.find((candidate) => chipKey(candidate) === next.chip) : undefined;
    if (item && chip) choose(item, chip);
    else if (item) select(item);
  }

  /** → expands the selected branch, or enters its timeline when expanded; ← steps from a
   * timeline entry back to its branch, or folds an expanded branch. */
  function step(by: number) {
    const selected = m.selected;
    if (selected === null) return;
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

  /** Move focus to the Pane's entries, the Main area or the Detail; the Pane shows first when
   * it is hidden. */
  async function focus(to: Focus) {
    if (to === "pane") app.paneShown = true;
    m.focus = to;
    await tick();
    panes[to]?.focus();
  }

  /** What a snapshot changes here, as it is accepted. */
  function accept(next: BrowseView) {
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (next.replaced) {
      m.opened = null;
      app.forgetReviews();
      if (m.focus === "detail") m.focus = "main";
    }
    // Leaving the shown Run (or losing it) closes its Detail; so does losing the node, which the
    // Detail finds as it reads its row.
    const opened = m.opened;
    if (opened?.source.kind === "run" && (next.run.kind !== "read" || opened.source.run !== next.run.id)) m.opened = null;
    // A Run of the branch whose Current is shown ended: scan it again.
    for (const item of next.branches) {
      const now = item.chips.flatMap((chip) => (chip.kind === "run" && chip.running ? [chip.id] : []));
      const before = m.running[keyOf(item.key)];
      m.running[keyOf(item.key)] = now;
      const ended = before !== undefined && before.some((id) => !now.includes(id));
      if (ended && m.shown.kind === "current" && keyOf(m.shown.branch) === keyOf(item.key)) void current.scan(item.key);
    }
    start(next);
  }

  /** Open what the launch asked for, once, on the first snapshot read. */
  function start(next: BrowseView) {
    if (m.started || !next.refreshed || !app.launch) return;
    m.started = true;
    const launched = next.launch === null ? undefined : next.branches.find((item) => keyOf(item.key) === keyOf(next.launch!));
    const mode = app.launch.mode;
    // A review's launch opens Review, which takes it.
    if (mode === "review") return;
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

  /** Esc in the Main area closes its Detail, else steps back to the Pane. */
  function back() {
    if (m.opened) closeDetail();
    else void focus("pane");
  }


  function mainKey(event: KeyboardEvent) {
    if (!plain(event, "Escape")) return;
    back();
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
      m.detailWidth = clampWidth(room.right - moved.clientX, room.width);
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
    if (shown.kind === "run") return shown.run;
    if (shown.kind === "current") return branch ? `Current · ${branch.project}${branch.name ? `  ${branch.name}` : ""}` : "Current";
    return "Nothing chosen";
  });
  // A refused edit on a Current's graph says why here, before what else went wrong: it is what
  // the person just did (D-CUR-7).
  const notice = $derived((shown.kind === "current" ? definitions.notice : null) ?? browse.error ?? view?.notice ?? null);

  useCommands(() => [
    { id: "main.run", group: "Graph", label: "Open the Run's Detail", enabled: () => run !== null, run: openRun },
    {
      id: "main.close",
      group: "Graph",
      label: "Close the Detail",
      keys: "Esc",
      enabled: () => m.opened !== null,
      run: closeDetail,
    },
  ], () => app.mode === "verify" && !app.monitor.settings);

  /** The graph's top within the stage, kept as `--graph-top`: the Detail docks from there, below
   * the Main area's header and below what the view shows above its graph (a Run's headline, a
   * Current's scan header). The graph's box fills the rest of the area, so a header that grows
   * resizes it and the top is read again; a mutation only looks for a new graph box. */
  const graphTop: Attachment<HTMLElement> = (stage) => {
    let watched: Element | null = null;
    const read = () => {
      if (!watched) {
        stage.style.removeProperty("--graph-top");
        return;
      }
      const top = watched.getBoundingClientRect().top - stage.getBoundingClientRect().top;
      stage.style.setProperty("--graph-top", `${top}px`);
    };
    const sized = new ResizeObserver(read);
    const find = () => {
      const area = stage.querySelector("[data-graph-area]");
      if (area === watched) return;
      if (watched) sized.unobserve(watched);
      watched = area;
      if (area) sized.observe(area);
      // A Detail can already be open when the lazy graph first mounts. Dock against
      // that layout now; the resize observer follows any later changes.
      read();
    };
    sized.observe(stage);
    const changed = new MutationObserver(find);
    changed.observe(stage, { childList: true, subtree: true });
    find();
    return () => {
      sized.disconnect();
      changed.disconnect();
    };
  };

  /** The Pane's narrowest share of the window, in percent. */
  const PANE_MIN = 12;
  /** The window's width, as the Pane and the Main area share it. */
  let windowWidth = $state(window.innerWidth);
  /** The Main area's narrowest share: room for the narrowest Detail beside the graph it keeps
   * visible (D-INS-1), and never less than 30 percent. */
  const mainMin = $derived(Math.min(100 - PANE_MIN, Math.max(30, ((MIN + KEEP) / windowWidth) * 100)));
</script>

<svelte:window bind:innerWidth={windowWidth} />

<div class="monitor">
  <PaneGroup direction="horizontal" autoSaveId="monitor-layout" class="columns">
    {#if app.paneShown}
      <Split id="pane" order={1} defaultSize={24} minSize={PANE_MIN}>
        <Pane
          bind:query={() => query, setQuery}
          {filled}
          onselect={open}
          ontoggle={toggle}
          onchoose={choose}
          onmove={move}
          onstep={step}
          onmain={() => void focus("main")}
        />
      </Split>
      <PaneResizer class="resizer" />
    {/if}
    <Split id="main" order={2} defaultSize={app.paneShown ? 76 : 100} minSize={mainMin}>
      <!-- The dragged width is the stage's, so the Detail, the Navigator beside it and a review's
           side column all take it. -->
      <div
        class="stage"
        style:--detail-width={m.detailWidth === null ? null : `${m.detailWidth}px`}
        style:--detail-cover={m.opened ? "var(--dock-width)" : null}
        {@attach graphTop}
      >
        <Panel
          {title}
          label="Main area"
          focused={m.focus === "main"}
          {notice}
          bind:noticeOpen={m.noticeOpen}
          bind:element={panes.main}
          onfocus={() => (m.focus = "main")}
          onkeydown={mainKey}
        >
          {#snippet lead()}
            {#if shown.kind === "run" && run?.origin?.kind === "chat"}
              {@const origin = run.origin}
              <!-- The way back, first: the Chat session that started the Run (J-RUN-ORIGIN). -->
              <Reference chip onclick={() => app.showChat(origin.sessionId, run.id)}>Chat · {origin.title}</Reference>
            {/if}
          {/snippet}
          {#snippet headline()}
            {#if shown.kind === "run" && run}
              <RunHeadline {run} onrun={openRun} />
            {:else if shown.kind === "current"}
              <ScanState branch={shown.branch} />
            {/if}
          {/snippet}
          {#snippet actions()}
            {#if shown.kind === "current"}
              <button class="button small" aria-label="Refresh (Ctrl+R)" aria-keyshortcuts="Control+R F5" onclick={() => void current.scan(shown.branch, true)}>Refresh</button>
            {/if}
          {/snippet}
          {#if shown.kind === "run"}
            {#if run}
              <RunView {run} branch={shown.branch} {revision} opened={m.opened?.node ?? null} onopen={openNode} onclose={closeDetail} />
            {:else if view?.run.kind === "missing"}
              <div class="pending"><GraphTools /><p class="empty">This Run cannot be read; choose another.</p></div>
            {:else}
              <div class="pending"><GraphTools /><p class="empty">Loading the Run…</p></div>
            {/if}
          {:else if shown.kind === "current"}
            <CurrentView branch={shown.branch} opened={m.opened?.node ?? null} onopen={openNode} onclose={closeDetail} />
          {:else}
            <p class="empty">{app.paneShown ? "Choose a branch in the Pane." : "Show the Pane with the Pane switch to choose a branch."}</p>
          {/if}
        </Panel>
        {#if m.opened && source && keyOf(m.opened.source) === keyOf(source)}
          <aside class="dock" aria-label="Detail">
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
              active={!m.settings && app.mode === "verify"}
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

<EditDialogs
  branch={shown.kind === "current" ? shown.branch : null}
  ondeleted={() => {
    // Delete is offered only in the open Detail's Definition: that Artifact is gone.
    closeDetail();
  }}
/>


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
  /* The Detail's width, as the dock takes it: a drag's, or its default, within its bounds. */
  .stage {
    --dock-width: clamp(min(320px, 100%), var(--detail-width, min(46%, 680px)), calc(100% - 240px));
    position: relative;
    isolation: isolate;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* The inspector: a panel docked at the graph's right edge, panel meeting panel at a line,
   * with the graph visible on its left. It sets its own width, by default or by a drag, within
   * the graph's area (as detail-width.ts fits a drag); its content never sizes it, and wraps or
   * scrolls in its own boxes. */
  /* From the graph's top (--graph-top, below the Main area's header and the view's own header
   * above its graph) to the area's bottom; under the header row alone until a graph is drawn. */
  .dock {
    position: absolute;
    z-index: 20;
    top: var(--graph-top, 32px);
    right: 0;
    bottom: 0;
    width: var(--dock-width);
    contain: inline-size;
    overflow: hidden;
    border-top: 1px solid var(--line-group);
    border-left: 1px solid var(--line-panel);
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
  /* What the Main area has to say where its graph would be: one line at the top of its body. */
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  /* A chosen Run with nothing to act on yet: the tool strip in its place, the line below it. */
  .pending {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
</style>
