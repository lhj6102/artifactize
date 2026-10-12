<script lang="ts">
  // Scope → Runs → Run tree → Detail as resizable panes. Focus moves between the panes; the
  // Detail follows the tree's selection as a peek until it is opened.
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BrowseView } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { MonitorState, setMonitorState, type Focus } from "./monitor.svelte";
  import { find } from "./tree.svelte";
  import ScopePane from "./ScopePane.svelte";
  import RunsPane from "./RunsPane.svelte";
  import RunPane from "./RunPane.svelte";
  import DetailPane from "./DetailPane.svelte";
  import { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";

  let { active }: { active: boolean } = $props();
  const { browse } = getAppState();

  const m = new MonitorState();
  setMonitorState(m);
  const { tree, reviews, panes } = m;

  const view = $derived(browse.view);
  const run = $derived(view?.run ?? null);
  const selectedNode = $derived(tree.selected.at(-1) ?? null);

  /** What a snapshot changes here, as it is accepted. */
  function accept(shown: BrowseView) {
    const at = shown.run;
    if (at) tree.sync(at.id, at.tree);
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (shown.replaced) {
      m.opened = null;
      for (const id of Object.keys(reviews)) delete reviews[id];
      if (m.focus === "detail") m.focus = "tree";
    }
    // Leaving the selected Run (or losing it) closes its Detail.
    if (m.opened !== null && m.opened !== at?.id) m.opened = null;
    // Another Run's graph starts zoomed out.
    const id = at?.id ?? null;
    if (id !== m.zoomRun) {
      m.zoomRun = id;
      m.zoomed = null;
    }
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });

  /** The Review of a request, if one was opened: rendering only looks. */
  const review = (id: string): Review | undefined => reviews[id];
  /** Open the Review of a request when its Detail arrives, never while rendering. */
  function openReview(id: string): void {
    reviews[id] ??= new Review(id, false);
  }

  /** Move focus to a pane's list, tree or review, or else to the pane itself. */
  async function move(to: Focus) {
    m.focus = to;
    await tick();
    const pane = panes[to];
    (pane?.querySelector<HTMLElement>('[tabindex="0"]') ?? pane)?.focus();
  }

  function openDetail() {
    if (!run || !selectedNode) return;
    m.opened = run.id;
    void move("detail");
  }

  function leaveDetail() {
    m.opened = null;
    void move("tree");
  }

  /** `!`: the next ERROR, RED or waiting Human eval; from Detail, open its Detail. */
  function attention() {
    if (!run) {
      m.notice = "Open a Run to find what needs attention.";
      return;
    }
    const fromDetail = m.opened !== null;
    if (!tree.nextAttention(run.tree)) {
      m.notice = "Nothing in this Run needs attention.";
      return;
    }
    m.notice = null;
    if (fromDetail) openDetail();
    else void move("tree");
  }

  /** Screen-wide: `!` works wherever focus is while the Monitor shows. */
  function keydown(event: KeyboardEvent) {
    if (!active || event.defaultPrevented) return;
    if (plain(event, "!")) {
      event.preventDefault();
      attention();
    }
  }

  /** The Runs pane has focus first, as in the terminal monitor. */
  const begin: Attachment = () => {
    if (active && !m.started && view?.refreshed) {
      m.started = true;
      void move(m.focus);
    }
  };

  /** `artifactize › scope › Run › node`, up to the focused level, as in the terminal. */
  const crumbs = $derived.by(() => {
    const parts = [view?.scopeLabel ?? "…"];
    if (m.focus !== "scope" && view?.selectedRun) parts.push(view.selectedRun);
    if ((m.focus === "tree" || m.focus === "detail") && run) {
      const node = find(run.tree, tree.selected);
      if (node) parts.push(node.kind === "run" ? "Run" : node.name);
    }
    return parts;
  });

  useCommands(() =>
    active
      ? [
          { id: "monitor.refresh", group: "Monitor", label: "Refresh", keys: "Ctrl+R", run: () => browse.refresh() },
          { id: "monitor.attention", group: "Monitor", label: "Next attention", keys: "!", run: attention },
          { id: "monitor.scope", group: "Monitor", label: "Focus Scope", run: () => void move("scope") },
          { id: "monitor.runs", group: "Monitor", label: "Focus Runs", run: () => void move("runs") },
          { id: "monitor.tree", group: "Monitor", label: "Focus Run tree", run: () => void move("tree") },
          {
            id: "monitor.view",
            group: "Monitor",
            label: m.runView === "tree" ? "Show the Run graph" : "Show the Run tree",
            keys: "v",
            run: () => {
              m.runView = m.runView === "tree" ? "graph" : "tree";
              void move("tree");
            },
          },
          {
            id: "monitor.open",
            group: "Monitor",
            label: "Open Detail of the selected row",
            keys: "Enter",
            enabled: () => selectedNode !== null,
            run: openDetail,
          },
        ]
      : [],
  );
</script>

<svelte:window onkeydown={keydown} />

<div class="monitor" {@attach begin}>
  <nav class="crumbs" aria-label="Breadcrumb">
    <span class="root">artifactize</span>
    {#each crumbs as crumb, index (index)}
      <span class="sep">›</span><span>{crumb}</span>
    {/each}
    {#if m.notice || browse.error || view?.error}
      <span class="notice" role="status">{m.notice ?? browse.error ?? view?.error}</span>
    {/if}
  </nav>
  <PaneGroup direction="horizontal" autoSaveId="monitor-columns" class="columns">
    <Pane defaultSize={24} minSize={12}>
      <PaneGroup direction="vertical" autoSaveId="monitor-sidebar">
        <Pane defaultSize={38} minSize={15}>
          <ScopePane
            focused={m.focus === "scope"}
            bind:element={panes.scope}
            onfocus={() => (m.focus = "scope")}
            onnext={() => void move("runs")}
          />
        </Pane>
        <PaneResizer class="resizer" />
        <Pane defaultSize={62} minSize={15}>
          <RunsPane
            focused={m.focus === "runs"}
            bind:element={panes.runs}
            onfocus={() => (m.focus = "runs")}
            onnext={() => void move("tree")}
            onback={() => void move("scope")}
          />
        </Pane>
      </PaneGroup>
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={36} minSize={18}>
      <RunPane
        {tree}
        bind:view={m.runView}
        bind:zoomed={m.zoomed}
        {active}
        focused={m.focus === "tree"}
        bind:element={panes.tree}
        onfocus={() => (m.focus = "tree")}
        onopen={openDetail}
        onback={() => void move("runs")}
        onattention={attention}
      />
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={40} minSize={20}>
      <DetailPane
        run={run?.id ?? null}
        node={selectedNode}
        full={m.opened !== null}
        focused={m.focus === "detail"}
        {active}
        {review}
        {openReview}
        bind:element={panes.detail}
        onfocus={() => (m.focus = "detail")}
        onopen={openDetail}
        onback={leaveDetail}
      />
    </Pane>
  </PaneGroup>
</div>

<style>
  .monitor {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100%;
  }
  .crumbs {
    display: flex;
    gap: 8px;
    align-items: baseline;
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
    color: var(--red);
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  :global(.columns) {
    min-height: 0;
  }
</style>
