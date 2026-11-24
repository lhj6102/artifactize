<script lang="ts">
  // Verify: the Pane on the left and the Main area on the right, with the divider between them
  // while the Pane is shown; the bar's Pane switch (Ctrl+B) hides both, the Main area taking the
  // workspace's width, and shows them again at the width the Pane had. A chip shows its Run in
  // the Main area, under its headline. A click on an Artifact (or the Run's headline) opens
  // its Detail, a panel docked at the graph's right
  // edge under its tool strip; dragging its left edge sets its width, kept like the split ratio.
  import { PaneGroup, Pane as Split, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { BranchKey, BranchView, BrowseView, Chip } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { chipKey, entryKey, keyOf, sourceOf, type Entry, type Focus } from "./monitor-state.svelte";
  import { mainSplitMinimum, PANE_MIN } from "./detail-width";
  import Pane from "./Pane.svelte";
  import Stage from "../shell/Stage.svelte";
  import RunView from "./RunView.svelte";
  import GraphTools from "./GraphTools.svelte";
  import GraphStage from "./GraphStage.svelte";
  import DetailPane from "./DetailPane.svelte";
  import type { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";
  import Reference from "../shell/Reference.svelte";
  import Icon from "../shell/Icon.svelte";

  const app = getAppState();
  const { browse, monitor: m } = app;
  const { panes } = m;

  const view = $derived(browse.view);
  const branches = $derived(view?.branches ?? []);
  const shown = $derived(m.shown);
  const run = $derived(shown.kind === "run" && view?.run.kind === "read" && view.run.id === shown.run && m.runRead?.id !== shown.run ? view.run : null);
  const source = $derived(sourceOf(shown));
  /** The shown branch, also while the Pane's search leaves it out. */
  const branch = $derived(shown.kind === "none" ? null : browse.branch(shown.branch));
  const revision = $derived(browse.revision);
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
    if (shown.kind !== "run" || keyOf(shown.branch) !== keyOf(item.key)) return null;
    const at = item.chips.findIndex((chip) => chip.id === shown.run);
    return at < 0 ? null : at;
  }

  function closeDetailOf(next: string | null) {
    if (m.opened && keyOf(m.opened.source) !== next) m.opened = null;
  }

  /** Choose the destination immediately. Its contents only follow a matching Browse reply. */
  let choosing = 0;
  async function navigate(item: BranchView, chip: Chip | null, selected: Entry) {
    const choice = ++choosing;
    if (chip && run?.id === chip.id) {
      m.selected = selected;
      return;
    }
    m.runRead = chip ? { id: chip.id, phase: "loading" } : null;
    m.finding = false;
    m.shown = chip ? { kind: "run", branch: item.key, run: chip.id } : { kind: "branch", branch: item.key };
    closeDetailOf(chip ? keyOf({ kind: "run", run: chip.id }) : null);
    m.selected = selected;
    const accepted = await (chip ? browse.open(chip.id) : browse.close());
    if (!chip || choice !== choosing || m.shown.kind !== "run" || m.shown.run !== chip.id) return;
    m.runRead = accepted && browse.view?.run.kind === "read" && browse.view.run.id === chip.id
      ? null : { id: chip.id, phase: "unreadable" };
  }

  function choose(item: BranchView, chip: Chip) {
    void navigate(item, chip, { kind: "chip", branch: item.key, chip: chipKey(chip) });
  }

  function select(item: BranchView) {
    void navigate(item, item.newest, { kind: "branch", branch: item.key });
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
      const first = item.newest;
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
    if (mode === "graph") {
      const target = next.launch ?? app.launch.graphBranch;
      if (launched) select(launched);
      if (target) void app.showBranchGraph(target);
      return;
    }
    if (launched) select(launched);
    void focus("pane");
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });

  let queryRevision = 0;
  function setQuery(text: string) {
    const typed = ++queryRevision;
    query = text;
    clearTimeout(typing);
    typing = setTimeout(
      () =>
        void browse.search(text).then((searched) => {
          if (!searched && typed === queryRevision) query = browse.query;
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

  /** The Run's Detail is the one open: Run details shows it on. */
  const runOpen = $derived(run !== null && m.opened !== null && m.opened.node === run.node);

  /** Run details opens the Run's Detail, and closes it again. */
  function toggleRun() {
    if (runOpen) closeDetail();
    else openRun();
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

  const notice = $derived((run ? m.graph.error : null) ?? browse.error ?? view?.notice ?? null);

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

  /** The Pane's narrowest share of the window, in percent. */
  /** The window's width, as the Pane and the Main area share it. */
  /** The Main area's narrowest share: room for the narrowest Detail beside the graph it keeps
   * visible (D-INS-1), and never less than 30 percent. */
  const mainMin = $derived(mainSplitMinimum(app.windowWidth));
</script>

{#snippet detail()}
  {#if m.opened}
    <DetailPane
      source={m.opened.source}
      {revision}
      {branch}
      {run}
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
  {/if}
{/snippet}


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
      <GraphStage detail={m.opened && source && keyOf(m.opened.source) === keyOf(source) ? detail : undefined}>
        <Stage
          label="Main area"
          {notice}
          bind:noticeOpen={m.noticeOpen}
          bind:element={panes.main}
          onfocus={() => (m.focus = "main")}
          onkeydown={mainKey}
        >
          {#snippet back()}
            {#if shown.kind === "run" && run?.origin?.kind === "chat"}
              {@const origin = run.origin}
              <!-- The way back: the Chat session that started the Run (J-RUN-ORIGIN). -->
              <Reference back onclick={() => app.showChat(origin.sessionId, run.id)}>Chat · {origin.title}</Reference>
            {/if}
          {/snippet}
          {#snippet tools()}
            {#if shown.kind === "run"}
              <span class="tool-sep"></span>
              <button class="tool" aria-pressed={runOpen} disabled={!run} onclick={toggleRun}>Run details</button>
              <button class="tool" disabled={!run} onclick={() => void app.showBranchGraph(run?.chatBranch ?? shown.branch)}><Icon name="open" />Open in Chat</button>
            {/if}
          {/snippet}
          {#if shown.kind === "run"}
            {#if run}
              <RunView {run} branch={shown.branch} {revision} opened={m.opened?.node ?? null} onopen={openNode} onclose={closeDetail} />
            {:else if (m.runRead?.id === shown.run && m.runRead.phase === "unreadable") || (!m.runRead && view?.run.kind === "missing")}
              <div class="pending"><GraphTools /><p class="empty">This Run cannot be read; choose another.</p></div>
            {:else}
              <div class="pending"><GraphTools /><p class="empty">Loading the Run…</p></div>
            {/if}
          {:else if shown.kind === "branch"}
            <div class="empty branch-empty">
              <span>No Runs on this branch yet.</span>
              <Reference onclick={() => void app.showBranchGraph(shown.branch)}>Open in Chat</Reference>
            </div>
          {:else}
            <p class="empty">{app.paneShown ? "Choose a branch in the Pane." : "Show the Pane from the command palette to choose a branch."}</p>
          {/if}
        </Stage>
      </GraphStage>
    </Split>
  </PaneGroup>
</div>

<style>
  .monitor {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  :global(.columns) {
    min-height: 0;
  }
  /* What the Main area has to say where its graph would be: one line at the top of its body. */
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  .branch-empty {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  /* A chosen Run with nothing to act on yet: the tool strip in its place, the line below it. */
  .pending {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
</style>
