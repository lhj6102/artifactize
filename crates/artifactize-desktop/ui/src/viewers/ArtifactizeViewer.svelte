<script lang="ts">
  // The Artifactize Viewer (J-VIEWER-ARTIFACTIZE, D-VIEW-5): what the project declares under the
  // Artifact, not a Run: a node per declared Artifact and its references as plain connections,
  // panned and zoomed as Verify's graph is. A click on an Artifact opens its panel at the right:
  // its evals with their kind, title, criteria and the Artifacts they read. While pointing, a
  // click on an Artifact or an eval appends its ID.
  import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from "@xyflow/svelte";
  import "@xyflow/svelte/dist/style.css";
  import { onMount } from "svelte";
  import { call, message } from "../ipc/client";
  import { layered } from "../monitor/layout";
  import { criteria, declared, edges as connections, evalsOf, type Declared } from "./declared";
  import DeclaredNode from "./DeclaredNode.svelte";
  import { getViewerState } from "./viewer-state.svelte";

  const v = getViewerState();
  const SIZE = { width: 168, height: 36 };
  const nodeTypes = { declared: DeclaredNode };

  let graph = $state.raw<Declared | null>(null);
  let error = $state<string | null>(null);
  let selected = $state<string | null>(null);

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
          data: { name, kind: artifact.kind, selected: selected === name, pointing: v.pointing },
          width: SIZE.width,
          height: SIZE.height,
        }))
      : [],
  );
  let edges = $derived<Edge[]>(graph ? connections(graph).map(({ source, target }) => ({ id: `${source}->${target}`, source, target, style: "stroke: var(--edge)" })) : []);
  const shown = $derived(graph && selected ? { name: selected, kind: graph.artifacts[selected]?.kind ?? "", evals: evalsOf(graph, selected) } : null);

  function clicked(name: string) {
    if (v.pointing) void v.point(name);
    else selected = selected === name ? null : name;
  }

  function pointEval(event: MouseEvent, id: string) {
    if (!v.pointing) return;
    event.preventDefault();
    void v.point(id);
  }

  onMount(async () => {
    try {
      graph = declared(await call(v.ipc.viewerDeclarations(v.session)));
    } catch (failure) {
      error = message(failure);
    }
  });
</script>

<div class="declared" class:open={shown !== null}>
  <div class="canvas" aria-label="Declared graph">
    {#if error}<p class="error">{error}</p>{/if}
    {#if graph}
      <SvelteFlowProvider>
        <SvelteFlow
          bind:nodes
          bind:edges
          {nodeTypes}
          fitView
          minZoom={0.1}
          maxZoom={2}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          proOptions={{ hideAttribution: true }}
          onnodeclick={({ node }) => clicked(node.id)}
        />
      </SvelteFlowProvider>
    {/if}
  </div>
  {#if shown}
    <section class="panel" aria-label="Artifact {shown.name}">
      <header><span class="name">{shown.name}</span><span class="tag">{shown.kind || "custom"}</span></header>
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

<style>
  .declared {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .declared.open {
    grid-template-columns: minmax(0, 1fr) min(420px, 45%);
  }
  .canvas {
    position: relative;
    min-width: 0;
    min-height: 0;
    background: var(--surface-app);
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: var(--surface-app);
    --xy-edge-stroke: var(--edge);
  }
  /* The Artifact's panel, docked at the right as the Detail is. */
  .panel {
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
    outline: 2px solid var(--accent);
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
