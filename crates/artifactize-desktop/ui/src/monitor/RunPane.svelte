<script lang="ts">
  // The selected Run: its headline, what needs attention, and the Artifacts and evals as a tree
  // or as a graph of Artifacts (`v` switches; the tree is the fully keyboard-driven view).
  import { ContextMenu } from "bits-ui";
  import type { Browse } from "./browse.svelte";
  import { type Tree, visible, find } from "./tree.svelte";
  import Panel from "../shell/Panel.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    browse,
    tree,
    focused,
    view = $bindable("tree"),
    zoomed = $bindable(null),
    element = $bindable(),
    onfocus,
    onopen,
    onback,
    onattention,
  }: {
    browse: Browse;
    tree: Tree;
    focused: boolean;
    /** The tree, or the graph of Artifacts. */
    view?: "tree" | "graph";
    /** The Artifact the graph is zoomed into. */
    zoomed?: string | null;
    element?: HTMLElement;
    onfocus: () => void;
    onopen: () => void;
    onback: () => void;
    onattention: () => void;
  } = $props();

  const run = $derived(browse.view?.run ?? null);
  const nodes = $derived(run?.tree ?? []);
  const rows = $derived(visible(nodes, tree.opened));
  const upstream = $derived(tree.upstream(nodes));
  const selectedId = $derived(tree.selected.join("/"));
  let list = $state<HTMLElement>();
  let offscreen = $state({ above: 0, below: 0 });

  const rowId = (path: string[]) => `tree-${path.join("/").replace(/[^A-Za-z0-9_-]/g, "_")}`;

  $effect(() => {
    void selectedId;
    list?.querySelector<HTMLElement>(`[data-path="${CSS.escape(selectedId)}"]`)?.scrollIntoView({ block: "nearest" });
  });

  /** Count marked upstream rows outside the visible part of the tree. */
  function measure() {
    if (!list) return;
    const view = list.closest(".body")?.getBoundingClientRect();
    if (!view) return;
    let above = 0;
    let below = 0;
    for (const row of list.querySelectorAll<HTMLElement>("[data-upstream]")) {
      const box = row.getBoundingClientRect();
      if (box.bottom <= view.top) above += 1;
      else if (box.top >= view.bottom) below += 1;
    }
    offscreen = { above, below };
  }

  $effect(() => {
    void upstream;
    void rows;
    queueMicrotask(measure);
  });

  function keydown(event: KeyboardEvent) {
    if (plain(event, "v")) view = view === "tree" ? "graph" : "tree";
    else if (view === "graph") {
      // The graph handles its own keys; what reaches here steps back or refreshes.
      if (plain(event, "Escape")) onback();
      else if (plain(event, "r")) browse.refresh();
      else return;
    } else if (plain(event, "ArrowDown") || plain(event, "j")) tree.move(rows, 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) tree.move(rows, -1);
    else if (plain(event, "h")) tree.left(nodes);
    else if (plain(event, "l")) tree.right(nodes);
    else if (plain(event, " ")) tree.toggle(nodes);
    else if (plain(event, "b")) tree.jumpBlocker(nodes);
    else if (plain(event, "Backspace")) tree.jumpBack();
    else if (plain(event, "Enter") || plain(event, "o") || plain(event, "ArrowRight")) onopen();
    else if (plain(event, "ArrowLeft") || plain(event, "Escape")) onback();
    else if (plain(event, "r")) browse.refresh();
    else return;
    event.preventDefault();
  }

  const selectedNode = $derived(find(nodes, tree.selected));
</script>

<Panel title={run ? `Run ${run.id}` : "Run"} {focused} bind:element {onfocus} onkeydown={keydown}>
  {#snippet actions()}
    <div class="views" role="group" aria-label="Run view">
      <button class="button small" aria-pressed={view === "tree"} onclick={() => (view = "tree")} title="Artifacts and evals (v)">Tree</button>
      <button class="button small" aria-pressed={view === "graph"} onclick={() => (view = "graph")} title="Graph of Artifacts (v)">Graph <kbd>v</kbd></button>
    </div>
  {/snippet}
  {#if !run}
    <p class="empty">Select a Run.</p>
  {:else if view === "graph"}
    <div class="graphmode">
    <div class="headline">
      <span class="status {tone(run.mark.status)}">{run.mark.glyph} {run.mark.status}</span>
      <span class="text">{run.headline}</span>
    </div>
    <div class="graph">
      <!-- Loaded on first use: the graph's libraries stay out of the tree's start-up. -->
      {#await import("./RunGraph.svelte")}
        <p class="empty">Loading the graph…</p>
      {:then { default: RunGraph }}
        <RunGraph
          run={run.id}
          revision={browse.revision}
          tree={nodes}
          selected={tree.selected}
          bind:zoomed
          onselect={(path) => tree.select(path)}
          {onopen}
        />
      {/await}
    </div>
    </div>
  {:else}
    <div class="headline">
      <span class="status {tone(run.mark.status)}">{run.mark.glyph} {run.mark.status}</span>
      <span class="text">{run.headline}</span>
    </div>
    {#if run.attention.length > 0}
      <ul class="attention">
        {#each run.attention as line, at (at)}
          <li><span class={tone(line.mark.status)}>{line.mark.glyph}</span> {line.text}</li>
        {/each}
        {#if run.more > 0}
          <li class="more">+{run.more} more · open the Run row</li>
        {/if}
      </ul>
    {/if}
    {#if offscreen.above > 0}<div class="offscreen">↑{offscreen.above} above</div>{/if}
    <ContextMenu.Root>
      <ContextMenu.Trigger>
        {#snippet child({ props })}
          <ul
            {...props}
            class="tree"
            role="tree"
            aria-label="Artifacts and evals"
            tabindex="0"
            aria-activedescendant={rows.length ? rowId(tree.selected) : undefined}
            bind:this={list}
            onscroll={measure}
          >
            {#each rows as row (row.path.join("/"))}
              {@const node = row.node}
              {@const mark = upstream.get(node.id)}
              <li
                id={rowId(row.path)}
                role="treeitem"
                aria-level={row.depth + 1}
                aria-selected={row.path.join("/") === selectedId}
                aria-expanded={node.children.length ? tree.opened.has(node.id) : undefined}
                class="row weight-{node.weight}"
                class:selected={row.path.join("/") === selectedId}
                data-path={row.path.join("/")}
                data-upstream={mark ? "" : undefined}
                style:padding-left="{12 + row.depth * 20}px"
                onclick={() => tree.select(row.path)}
                ondblclick={() => {
                  tree.select(row.path);
                  onopen();
                }}
                oncontextmenu={() => tree.select(row.path)}
                onkeydown={() => {}}
              >
                <span class="twisty">
                  {#if node.children.length}
                    <button
                      class="fold"
                      tabindex="-1"
                      aria-label={tree.opened.has(node.id) ? "Fold" : "Unfold"}
                      onclick={(event) => {
                        event.stopPropagation();
                        tree.toggle(nodes, node.id);
                      }}>{tree.opened.has(node.id) ? "▾" : "▸"}</button
                    >
                  {/if}
                </span>
                <span class="glyph {tone(node.tone)}">{node.glyph}</span>
                <span class="name">{node.name}<span class="marks">{node.marks}</span></span>
                {#if mark}
                  <span class="up" class:met={mark.complete} title="An upstream Artifact of the selected row">↑</span>
                {/if}
                <span class="status">
                  {#each node.text as segment, at (at)}
                    <span class={tone(segment.tone)}>{segment.text}</span>
                  {/each}
                </span>
                <span class="right">{node.right}{#if node.changed}<span class="changed" title="Changed since the Run ended">*</span>{/if}</span>
              </li>
            {/each}
          </ul>
        {/snippet}
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content class="menu">
          <ContextMenu.Item class="menu-item" onSelect={onopen}>Open Detail <kbd>Enter</kbd></ContextMenu.Item>
          <ContextMenu.Item
            class="menu-item"
            disabled={!selectedNode?.children.length}
            onSelect={() => tree.toggle(nodes)}
          >
            {selectedNode && tree.opened.has(selectedNode.id) ? "Fold" : "Unfold"} <kbd>Space</kbd>
          </ContextMenu.Item>
          <ContextMenu.Item
            class="menu-item"
            disabled={!selectedNode?.upstream.length}
            onSelect={() => tree.jumpBlocker(nodes)}
          >
            Go to upstream Artifact <kbd>b</kbd>
          </ContextMenu.Item>
          <ContextMenu.Item class="menu-item" disabled={!tree.blocker} onSelect={() => tree.jumpBack()}>
            Back to the origin row <kbd>Backspace</kbd>
          </ContextMenu.Item>
          <ContextMenu.Separator class="menu-separator" />
          <ContextMenu.Item class="menu-item" onSelect={onattention}>Next attention <kbd>!</kbd></ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
    {#if offscreen.below > 0}<div class="offscreen">↑{offscreen.below} below</div>{/if}
  {/if}
</Panel>

<style>
  .views {
    display: flex;
    gap: 4px;
  }
  .views [aria-pressed="true"] {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .graphmode {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .graph {
    flex: 1;
    min-height: 240px;
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .headline {
    padding: 0 var(--pad) 6px;
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  .headline .status {
    font-weight: 600;
  }
  .headline .text {
    color: var(--muted);
  }
  .attention {
    list-style: none;
    margin: 0;
    padding: 0 var(--pad) 8px;
  }
  .attention li {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more,
  .offscreen {
    color: var(--muted);
    font-size: 12px;
  }
  .offscreen {
    padding: 2px var(--pad);
  }
  .tree {
    list-style: none;
    margin: 0;
    padding: 4px 0 12px;
    border-top: 1px solid var(--border);
    outline: none;
  }
  .row {
    display: grid;
    grid-template-columns: 16px 16px auto auto 1fr auto;
    gap: 6px;
    align-items: baseline;
    padding: 3px 12px;
    cursor: default;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  .tree:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .weight-bold .name,
  .weight-bold .status {
    font-weight: 600;
  }
  .weight-dim {
    color: var(--muted);
  }
  .fold {
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
    font: inherit;
  }
  .marks {
    color: var(--faint);
  }
  .up {
    color: var(--accent);
    font-weight: 700;
  }
  .up.met {
    color: var(--faint);
    font-weight: 400;
  }
  .status {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .right {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .changed {
    color: var(--faint);
    margin-left: 2px;
  }
</style>
