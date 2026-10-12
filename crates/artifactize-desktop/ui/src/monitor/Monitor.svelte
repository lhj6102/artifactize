<script lang="ts">
  // Scope → Runs → Run tree → Detail as resizable panes. Focus moves between the panes; the
  // Detail follows the tree's selection as a peek until it is opened.
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { tick } from "svelte";
  import type { Browse } from "./browse.svelte";
  import { Tree, find } from "./tree.svelte";
  import ScopePane from "./ScopePane.svelte";
  import RunsPane from "./RunsPane.svelte";
  import RunPane from "./RunPane.svelte";
  import DetailPane from "./DetailPane.svelte";
  import { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";

  let { browse, active }: { browse: Browse; active: boolean } = $props();

  type Focus = "scope" | "runs" | "tree" | "detail";
  let focus = $state<Focus>("runs");
  let opened = $state<{ run: string; node: string } | null>(null);
  let notice = $state<string | null>(null);
  const tree = new Tree();
  /** Human reviews by request: leaving and reopening a request keeps its drafts. */
  const reviews = new Map<string, Review>();
  const panes: Partial<Record<Focus, HTMLElement>> = $state({});

  const view = $derived(browse.view);
  const run = $derived(view?.run ?? null);
  const selectedNode = $derived(tree.selected.at(-1) ?? null);

  $effect(() => {
    if (run) tree.sync(run.id, run.tree);
  });

  $effect(() => {
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (view?.replaced) {
      opened = null;
      reviews.clear();
      if (focus === "detail") focus = "tree";
    }
  });

  $effect(() => {
    // Leaving the selected Run (or losing it) closes its Detail.
    if (opened && opened.run !== run?.id) opened = null;
  });

  function review(id: string): Review {
    let found = reviews.get(id);
    if (!found) {
      found = new Review(id, false);
      reviews.set(id, found);
    }
    return found;
  }

  /** Move focus to a pane's list, tree or review, or else to the pane itself. */
  async function move(to: Focus) {
    focus = to;
    await tick();
    const pane = panes[to];
    (pane?.querySelector<HTMLElement>('[tabindex="0"]') ?? pane)?.focus();
  }

  function openDetail() {
    if (!run || !selectedNode) return;
    opened = { run: run.id, node: selectedNode };
    void move("detail");
  }

  function leaveDetail() {
    opened = null;
    void move("tree");
  }

  /** `!`: the next ERROR, RED or waiting Human eval; from Detail, open its Detail. */
  function attention() {
    if (!run) {
      notice = "Open a Run to find what needs attention.";
      return;
    }
    const fromDetail = opened !== null;
    if (!tree.nextAttention(run.tree)) {
      notice = "Nothing in this Run needs attention.";
      return;
    }
    notice = null;
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

  let started = false;
  $effect(() => {
    // The Runs pane has focus first, as in the terminal monitor.
    if (active && !started && view?.refreshed) {
      started = true;
      void move(focus);
    }
  });

  /** `artifactize › scope › Run › node`, up to the focused level, as in the terminal. */
  const crumbs = $derived.by(() => {
    const parts = [view?.scopeLabel ?? "…"];
    if (focus !== "scope" && view?.selectedRun) parts.push(view.selectedRun);
    if ((focus === "tree" || focus === "detail") && run) {
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

<div class="monitor">
  <nav class="crumbs" aria-label="Breadcrumb">
    <span class="root">artifactize</span>
    {#each crumbs as crumb, index (index)}
      <span class="sep">›</span><span>{crumb}</span>
    {/each}
    {#if notice || browse.error || view?.error}
      <span class="notice" role="status">{notice ?? browse.error ?? view?.error}</span>
    {/if}
  </nav>
  <PaneGroup direction="horizontal" autoSaveId="monitor-columns" class="columns">
    <Pane defaultSize={24} minSize={12}>
      <PaneGroup direction="vertical" autoSaveId="monitor-sidebar">
        <Pane defaultSize={38} minSize={15}>
          <ScopePane
            {browse}
            focused={focus === "scope"}
            bind:element={panes.scope}
            onfocus={() => (focus = "scope")}
            onnext={() => void move("runs")}
          />
        </Pane>
        <PaneResizer class="resizer" />
        <Pane defaultSize={62} minSize={15}>
          <RunsPane
            {browse}
            focused={focus === "runs"}
            bind:element={panes.runs}
            onfocus={() => (focus = "runs")}
            onnext={() => void move("tree")}
            onback={() => void move("scope")}
          />
        </Pane>
      </PaneGroup>
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={36} minSize={18}>
      <RunPane
        {browse}
        {tree}
        focused={focus === "tree"}
        bind:element={panes.tree}
        onfocus={() => (focus = "tree")}
        onopen={openDetail}
        onback={() => void move("runs")}
        onattention={attention}
      />
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={40} minSize={20}>
      <DetailPane
        {browse}
        run={opened?.run ?? run?.id ?? null}
        node={opened?.node ?? selectedNode}
        full={opened !== null}
        focused={focus === "detail"}
        {active}
        {review}
        bind:element={panes.detail}
        onfocus={() => (focus = "detail")}
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
