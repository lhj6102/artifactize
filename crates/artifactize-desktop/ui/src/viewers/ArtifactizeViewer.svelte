<script lang="ts">
  // The Artifactize Viewer (J-VIEWER-ARTIFACTIZE, D-VIEW-5): what the project declares under the
  // Artifact, not a Run: a node per declared Artifact and its references as plain connections,
  // panned and zoomed as Verify's graph is. A click on an Artifact opens its panel at the right:
  // its evals with their kind, title, criteria and the Artifacts they read. While pointing, a
  // click on an Artifact or an eval appends its ID.
  import { SvelteFlow, useSvelteFlow, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { onMount } from "svelte";
  import GraphTools from "../monitor/GraphTools.svelte";
  import { layered } from "../monitor/layout";
  import { composed, criteria, edges as connections, evalsOf } from "./declared";
  import DeclaredNode from "./DeclaredNode.svelte";
  import { getViewerState } from "./viewer-state.svelte";

  const v = getViewerState();
  const flow = useSvelteFlow();
  const MIN_ZOOM = 0.1;
  const MAX_ZOOM = 2;
  const SIZE = { width: 168, height: 36 };
  const nodeTypes = { declared: DeclaredNode };
  /** The smallest grid cell, in screen pixels, still drawn. */
  const GRID_SHOWN = 8;
  const state = v.project;
  const graph = $derived(state.graph);
  const selected = $derived(state.selected);
  const camera = $derived(state.camera);
  const cell = $derived(state.unit * camera.zoom);
  const gridCell: Attachment<HTMLElement> = (canvas) => {
    const measured = Number.parseFloat(getComputedStyle(canvas).getPropertyValue("--grid"));
    if (measured) state.unit = measured;
  };

  /** Where each declared Artifact is drawn. */
  const places = $derived.by(() => {
    if (!graph) return new Map<string, { x: number; y: number }>();
    const names = Object.keys(graph.artifacts);
    return layered({ nodes: names.map((id) => ({ id })), edges: connections(graph), sizes: new Map(names.map((name) => [name, SIZE])) });
  });
  let nodes = $derived<Node[]>(
    graph
      ? Object.entries(graph.artifacts).map(([name, artifact]) => ({
          id: name,
          type: "declared",
          position: places.get(name) ?? { x: 0, y: 0 },
          data: { name, kind: composed(artifact), selected: selected === name, pointing: v.pointing },
          width: SIZE.width,
          height: SIZE.height,
        }))
      : [],
  );
  let edges = $derived<Edge[]>(graph ? connections(graph).map(({ source, target }) => ({ id: `${source}->${target}`, source, target, style: "stroke: var(--edge)" })) : []);
  const shown = $derived(graph && selected ? { name: selected, kind: composed(graph.artifacts[selected]), evals: evalsOf(graph, selected) } : null);

  function clicked(name: string) {
    if (v.pointing) void v.point(name);
    else state.selected = selected === name ? null : name;
  }

  function pointEval(event: MouseEvent, id: string) {
    if (!v.pointing) return;
    event.preventDefault();
    void v.point(id);
  }

  onMount(() => void v.loadDeclarations());
</script>

<div class="declared">
  <GraphTools zoom={camera.zoom} showFind={false} act={graph ? {
    zoomBy: (factor) => void flow.setZoom(Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, flow.getZoom() * factor))),
    fit: () => void flow.fitView({ maxZoom: 1 }),
  } : null} />
  <div class="surface">
    <div
      class="canvas"
      class:far={cell < GRID_SHOWN}
      aria-label="Declared graph"
      style:background-size="{cell}px {cell}px"
      style:background-position="{camera.x}px {camera.y}px"
      {@attach gridCell}
    >
      {#if state.error}<p class="error">{state.error}</p>{/if}
      {#if graph}
        <SvelteFlow
          bind:nodes
          bind:edges
          bind:viewport={state.camera}
          {nodeTypes}
          fitView
          fitViewOptions={{ maxZoom: 1 }}
          minZoom={MIN_ZOOM}
          maxZoom={MAX_ZOOM}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          proOptions={{ hideAttribution: true }}
          onnodeclick={({ node }) => clicked(node.id)}
        />
      {/if}
    </div>
    {#if shown}
      <section class="panel" aria-label="Artifact {shown.name}">
        <header><span class="name">{shown.name}</span>{#if shown.kind}<span class="tag">{shown.kind}</span>{/if}</header>
        <div class="evals">
          {#each shown.evals as [id, each] (id)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <section class="eval" class:pointing={v.pointing} aria-label={id} onclick={(event) => pointEval(event, id)}>
              <p class="id"><span class="mono">{id}</span><span class="tag">{each.criteria.kind}</span></p>
              <p class="label">Title</p>
              <p>{each.title}</p>
              {#if criteria(each)}
                <p class="label">Criteria</p>
                <p class="criteria">{criteria(each)}</p>
              {/if}
              {#if each.reads?.length}
                <p class="label">Reads</p>
                <p class="reads">{#each each.reads as read (read)}<span class="tag">{read}</span>{/each}</p>
              {/if}
            </section>
          {:else}
            <p class="quiet">No evals.</p>
          {/each}
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  .declared {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .surface {
    position: relative;
  }
  /* The canvas keeps the whole window; its faint grid moves and scales with the camera, left
     out once its cells are too small to read (D-GRAPH-1, D-GRAPH-6). */
  .canvas {
    border-top: 1px solid var(--line-group);
    position: absolute;
    inset: 0;
    background-color: var(--surface-app);
    background-image:
      linear-gradient(to right, color-mix(in srgb, var(--line-group) 40%, transparent) 1px, transparent 1px),
      linear-gradient(to bottom, color-mix(in srgb, var(--line-group) 40%, transparent) 1px, transparent 1px);
  }
  .canvas.far {
    background-image: none;
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: transparent;
    --xy-edge-stroke: var(--edge);
  }
  /* The Artifact's panel, docked over the canvas's right edge as the Detail is (D-INS-1). */
  .panel {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(clamp(320px, 45%, 420px), max(320px, calc(100% - 240px)), 100%);
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
    border-left: 1px solid var(--line-panel);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--row-head);
    padding: 0 var(--pad);
    border-bottom: 1px solid var(--line-group);
  }
  .name {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
  }
  .evals {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .eval {
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--line-group);
  }
  .eval.pointing:hover {
    outline: 2px dashed var(--handle);
    box-shadow: inset 0 0 0 2px var(--handle-edge);
    outline-offset: -2px;
    cursor: crosshair;
  }
  .eval p {
    margin: 0 0 4px;
    overflow-wrap: anywhere;
  }
  .id {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .mono {
    font-family: var(--mono);
  }
  .label {
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .criteria {
    white-space: pre-wrap;
  }
  .reads {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag {
    padding: 0 4px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    color: var(--ink-2);
    font: var(--text-label) var(--mono);
  }
  .quiet {
    margin: 0;
    padding: 8px var(--pad);
    color: var(--ink-2);
  }
  .error {
    margin: 0;
    padding: 8px var(--pad);
    color: var(--red);
  }
</style>
