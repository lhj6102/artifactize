<script lang="ts">
  // The laid-out Run graph and, once an Artifact is zoomed into, its evals as the tree shows
  // them. Nodes are focusable: arrows move between connected Artifacts, Enter or a click zooms
  // in, Esc zooms out. An eval selected here is the tree's selection, so Detail follows it.
  import { SvelteFlow, Controls, MiniMap, useSvelteFlow, type Edge, type Node } from "@xyflow/svelte";
  import { tick } from "svelte";
  import type { RunGraph, TreeNode } from "../ipc/client";
  import ArtifactNode from "./ArtifactNode.svelte";
  import { LEGEND, WORDS, neighbor, type Arrow, type Point } from "./graph";
  import { NODE } from "./layout";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    graph,
    positions,
    tree,
    selected,
    zoomed = $bindable(null),
    onselect,
    onopen,
  }: {
    graph: RunGraph;
    positions: Map<string, Point>;
    /** The Run's tree rows: an Artifact's evals come from there. */
    tree: TreeNode[];
    selected: string[];
    zoomed?: string | null;
    onselect: (path: string[]) => void;
    onopen: () => void;
  } = $props();

  const flow = useSvelteFlow();
  const nodeTypes = { artifact: ArtifactNode };
  let canvas = $state<HTMLElement>();
  let list = $state<HTMLElement>();
  /** The eval of the zoomed Artifact the list points at. */
  let index = $state(0);

  let nodes = $state.raw<Node[]>([]);
  let edges = $state.raw<Edge[]>([]);

  $effect(() => {
    // Lights change in place; positions only come with a new layout.
    nodes = graph.nodes
      .filter((node) => positions.has(node.id))
      .map((node) => ({
        id: node.id,
        type: "artifact",
        position: positions.get(node.id)!,
        // A fixed size: recoloured nodes keep their dimensions and never re-measure.
        width: NODE.width,
        height: NODE.height,
        data: { name: node.name, light: node.light, zoomed: node.id === zoomed },
        selected: node.id === selected[0],
        draggable: false,
        connectable: false,
        ariaLabel: `${node.name}: ${WORDS[node.light]}`,
        domAttributes: { "data-light": node.light },
      }));
    edges = graph.edges.map((edge) => ({
      id: `${edge.source}>${edge.target}`,
      source: edge.source,
      target: edge.target,
      class: `light-${edge.light}`,
      selectable: false,
      focusable: false,
      domAttributes: { "data-light": edge.light },
    }));
  });

  const artifact = $derived(zoomed ? (tree.find((node) => node.id === zoomed) ?? null) : null);
  const evals = $derived(artifact?.children ?? []);

  function element(id: string): HTMLElement | null {
    return canvas?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(id)}"]`) ?? null;
  }

  async function zoomIn(id: string) {
    zoomed = id;
    const at = evals.findIndex((node) => node.id === selected[1]);
    index = Math.max(at, 0);
    onselect(evals[index] ? [id, evals[index].id] : [id]);
    void flow.fitView({ nodes: [{ id }], duration: 250, maxZoom: 1.6, padding: 0.6 });
    await tick();
    list?.focus();
  }

  async function zoomOut() {
    const id = zoomed;
    zoomed = null;
    if (id) onselect([id]);
    void flow.fitView({ duration: 250 });
    await tick();
    if (id) element(id)?.focus();
  }

  /** Keys on the canvas: arrows move along edges, Enter zooms in, Esc zooms out. */
  function keydown(event: KeyboardEvent) {
    const id = (event.target as HTMLElement | null)?.closest<HTMLElement>(".svelte-flow__node")?.dataset.id;
    if (!id) return;
    const arrows: Arrow[] = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    const arrow = arrows.find((key) => plain(event, key));
    if (arrow) {
      const next = neighbor(graph, positions, id, arrow);
      if (next) {
        onselect([next]);
        element(next)?.focus();
      }
    } else if (plain(event, "Enter")) void zoomIn(id);
    else if (plain(event, "Escape") && zoomed) void zoomOut();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function listKey(event: KeyboardEvent) {
    const move = (to: number) => {
      index = Math.min(Math.max(to, 0), evals.length - 1);
      if (zoomed && evals[index]) onselect([zoomed, evals[index].id]);
    };
    if (plain(event, "ArrowDown") || plain(event, "j")) move(index + 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) move(index - 1);
    else if ((plain(event, "Enter") || plain(event, "o") || plain(event, "ArrowRight")) && evals[index]) onopen();
    else if (plain(event, "Escape") || plain(event, "ArrowLeft")) void zoomOut();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<div class="graph" class:split={zoomed !== null}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="canvas" bind:this={canvas} onkeydown={keydown}>
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView
      minZoom={0.05}
      maxZoom={2}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={true}
      onlyRenderVisibleElements={graph.nodes.length > 80}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => void zoomIn(node.id)}
    >
      <Controls showLock={false} />
      {#if graph.nodes.length > 40}<MiniMap pannable zoomable />{/if}
    </SvelteFlow>
    <ul class="legend" aria-label="Legend">
      {#each LEGEND as light (light)}
        <li class="light-{light}"><span class="swatch"></span>{WORDS[light]}</li>
      {/each}
    </ul>
  </div>
  {#if artifact}
    <section class="evals" aria-label="Evals of {artifact.name}">
      <header>
        <strong>{artifact.name}</strong>
        <span class="muted">{artifact.right}</span>
        <button class="button small" onclick={() => void zoomOut()} title="Back to the whole graph">Zoom out <kbd>Esc</kbd></button>
      </header>
      <ul role="listbox" aria-label="Evals" tabindex="0" bind:this={list} onkeydown={listKey}>
        {#each evals as node, at (node.id)}
          <li
            role="option"
            aria-selected={at === index}
            class:selected={at === index}
            class="weight-{node.weight}"
            onclick={() => {
              index = at;
              if (zoomed) onselect([zoomed, node.id]);
            }}
            ondblclick={() => {
              index = at;
              if (zoomed) onselect([zoomed, node.id]);
              onopen();
            }}
            onkeydown={() => {}}
          >
            <span class={tone(node.tone)}>{node.glyph}</span>
            <span class="name">{node.name}<span class="muted">{node.marks}</span></span>
            <span class="status">
              {#each node.text as segment, part (part)}<span class={tone(segment.tone)}>{segment.text}</span>{/each}
            </span>
            <span class="muted">{node.right}</span>
          </li>
        {:else}
          <li class="muted">No evals.</li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .graph {
    height: 100%;
    display: grid;
    grid-template-rows: 1fr;
    min-height: 0;
  }
  .graph.split {
    grid-template-rows: minmax(140px, 1fr) minmax(120px, auto);
  }
  .canvas {
    position: relative;
    min-height: 0;
    border-top: 1px solid var(--border);
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: var(--bg);
    --xy-edge-stroke-width: 1.5;
  }
  .canvas :global(.svelte-flow__edge path) {
    stroke: var(--light, var(--faint));
  }
  .canvas :global(.svelte-flow__node:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    border-radius: var(--radius);
  }
  .legend {
    position: absolute;
    left: 8px;
    top: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin: 0;
    padding: 4px 8px;
    list-style: none;
    font-size: 11px;
    color: var(--muted);
    background: color-mix(in srgb, var(--surface) 85%, transparent);
    border-radius: 6px;
    pointer-events: none;
    z-index: 5;
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 4px;
    border-radius: 50%;
    background: var(--light);
  }
  .evals {
    border-top: 1px solid var(--border);
    overflow: auto;
    max-height: 50vh;
  }
  .evals header {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 6px var(--pad);
  }
  .evals header .button {
    margin-left: auto;
  }
  .evals ul {
    list-style: none;
    margin: 0;
    padding: 0 0 8px;
    outline: none;
  }
  .evals li {
    display: grid;
    grid-template-columns: 16px auto 1fr auto;
    gap: 8px;
    padding: 3px var(--pad);
    white-space: nowrap;
    cursor: default;
  }
  .evals li:hover {
    background: var(--surface-2);
  }
  .evals .selected {
    background: var(--selection);
  }
  .evals ul:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .status {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .muted {
    color: var(--muted);
  }
  .weight-bold .name,
  .weight-bold .status {
    font-weight: 600;
  }
  .weight-dim {
    color: var(--muted);
  }
</style>
