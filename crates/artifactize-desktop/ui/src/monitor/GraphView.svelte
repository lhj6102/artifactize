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
    // Lights change in place; positions only come with a new layout. The selection is not part
    // of a node: a node whose selection flips is drawn again and loses focus, so focus (which
    // follows the selection) marks the selected Artifact instead.
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

  $effect(() => {
    // The tree's selection moves the highlight; when it leaves the evals (a completed
    // Artifact folds in the tree), the highlight stays and Enter selects it again.
    if (!zoomed || selected[0] !== zoomed) return;
    const at = evals.findIndex((node) => node.id === selected[1]);
    if (at >= 0) index = at;
    else index = Math.min(index, Math.max(evals.length - 1, 0));
  });

  /** Open the Detail of the highlighted eval, which becomes the selection first. */
  function openEval() {
    if (!zoomed || !evals[index]) return;
    onselect([zoomed, evals[index].id]);
    onopen();
  }

  function element(id: string): HTMLElement | null {
    return canvas?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(id)}"]`) ?? null;
  }

  /** The node is drawn and wholly inside the canvas. */
  function inView(id: string): boolean {
    const box = element(id)?.getBoundingClientRect();
    const view = canvas?.getBoundingClientRect();
    return !!box && !!view && box.left >= view.left && box.right <= view.right && box.top >= view.top && box.bottom <= view.bottom;
  }

  /** Centre the camera on `id` at the current zoom unless it is wholly in view. */
  async function centre(id: string) {
    const at = positions.get(id);
    if (!at || inView(id)) return;
    const { zoom } = flow.getViewport();
    await flow.setCenter(at.x + NODE.width / 2, at.y + NODE.height / 2, { zoom, duration: 0 });
  }

  // Graph keyboard focus has one owner: `held` is the Artifact node that should have focus
  // while the graph has the keyboard. Camera moves and culling (only visible nodes are drawn
  // above 80 Artifacts) can remove the focused node and drop focus to the page; a watch puts it
  // back every frame, or on the canvas itself while the node is not drawn, so keys keep
  // reaching the graph. Each handoff is numbered; a newer one voids older ones. Focus that
  // moves anywhere else (the eval list, another pane, a press outside) ends the hold.
  let held: string | null = null;
  let handoff = 0;
  let watching = false;

  const lost = () => document.activeElement === null || document.activeElement === document.body;

  function watch() {
    if (watching) return;
    watching = true;
    const step = () => {
      if (held === null || !canvas?.isConnected) {
        watching = false;
        return;
      }
      const target = element(held);
      const active = document.activeElement;
      if (target && (lost() || active === canvas)) target.focus({ preventScroll: true });
      else if (!target && lost()) canvas.focus({ preventScroll: true });
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  /** Give graph focus to `id` from now on; returns this handoff's number. */
  function hold(id: string): number {
    held = id;
    handoff += 1;
    if (document.activeElement !== element(id)) element(id)?.focus({ preventScroll: true });
    watch();
    return handoff;
  }

  function release() {
    held = null;
    handoff += 1;
  }

  $effect(() => {
    // Focus or a press anywhere outside the graph's nodes and canvas ends the hold.
    const focusin = (event: FocusEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target || !canvas || target === canvas) return;
      const id = canvas.contains(target) ? target.closest<HTMLElement>(".svelte-flow__node")?.dataset.id : undefined;
      if (!id) return release();
      if (id !== held) hold(id);
      // Focus is the graph's selection, so the keys always start from the node in focus.
      if (id !== selected[0] && !zoomed) onselect([id]);
    };
    const pointerdown = (event: PointerEvent) => {
      if (!canvas?.contains(event.target as HTMLElement | null)) release();
    };
    document.addEventListener("focusin", focusin);
    document.addEventListener("pointerdown", pointerdown, true);
    return () => {
      document.removeEventListener("focusin", focusin);
      document.removeEventListener("pointerdown", pointerdown, true);
      release();
    };
  });

  async function zoomIn(id: string) {
    release();
    zoomed = id;
    const at = evals.findIndex((node) => node.id === selected[1]);
    index = Math.max(at, 0);
    onselect(evals[index] ? [id, evals[index].id] : [id]);
    void flow.fitView({ nodes: [{ id }], duration: 250, maxZoom: 1.6, padding: 0.6 });
    await tick();
    list?.focus();
  }

  /** Back to the whole graph, keyboard focus on the Artifact through the camera's move. */
  async function zoomOut() {
    const id = zoomed;
    zoomed = null;
    if (!id) return;
    onselect([id]);
    // Hold focus at once, so nothing during the transition can leave it on the page.
    await tick();
    const mine = hold(id);
    await flow.fitView({ duration: 250 });
    // Too many Artifacts to fit: bring this one into view, unless keys moved on meanwhile.
    if (mine === handoff) await centre(id);
  }

  /** The Artifact node `event` happened on. */
  function nodeOf(event: Event): string | undefined {
    return (event.target as HTMLElement | null)?.closest<HTMLElement>(".svelte-flow__node")?.dataset.id;
  }

  /**
   * Select `id` and give it keyboard focus, centring it first when it is not in view: with
   * many Artifacts only the visible ones are drawn, so a node beyond the view mounts only once
   * the camera reaches it; the hold focuses it then.
   */
  async function reveal(id: string) {
    onselect([id]);
    hold(id);
    await centre(id);
  }

  /** Keys on the canvas: arrows move along edges, Enter zooms in, Esc zooms out. */
  function keydown(event: KeyboardEvent) {
    // On a node, or on the canvas while the held node is not drawn.
    const focused = nodeOf(event) ?? (event.target === canvas ? (held ?? undefined) : undefined);
    if (!focused) return;
    // From the selected Artifact, which focus follows; the focused node if they differ.
    const id = selected[0] && positions.has(selected[0]) && !zoomed ? selected[0] : focused;
    const arrows: Arrow[] = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    const arrow = arrows.find((key) => plain(event, key));
    if (arrow) {
      const next = neighbor(graph, positions, id, arrow);
      if (next) void reveal(next);
    } else if (plain(event, "Enter")) void zoomIn(focused);
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
    else if ((plain(event, "Enter") || plain(event, "o") || plain(event, "ArrowRight")) && evals[index]) openEval();
    else if (plain(event, "Escape") || plain(event, "ArrowLeft")) void zoomOut();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<div class="graph" class:split={zoomed !== null}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="canvas" bind:this={canvas} onkeydown={keydown} tabindex="-1">
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
      autoPanOnNodeFocus={false}
      elevateNodesOnSelect={false}
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
              openEval();
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
