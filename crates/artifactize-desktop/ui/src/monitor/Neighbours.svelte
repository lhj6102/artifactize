<script lang="ts">
  // An Artifact between its neighbours, drawn as the Run graph draws them: the Artifacts its
  // evals use on the left, the ones whose evals use it on the right, with the graph's nodes and
  // edges. It holds still: no pan, zoom or drag; a click on a neighbour opens its Detail.
  import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, type RunGraph } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import ArtifactNode from "./ArtifactNode.svelte";
  import { NeighboursState, setNeighboursState } from "./neighbours-state.svelte";
  import { NODE } from "./layout";

  let { run, node, onopen }: { run: string; node: string; onopen: (node: string) => void } = $props();
  const commands = useIpc();
  const { browse } = getAppState();
  const nodeTypes = { artifact: ArtifactNode };
  /** Room between columns and between stacked neighbours. */
  const GAP = { x: 72, y: 16 };

  const n = new NeighboursState();
  setNeighboursState(n);
  /** Read the graph again for every accepted snapshot: it may relight the neighbours. */
  const reread: Attachment<HTMLElement> = () => {
    void browse.revision;
    const at = run;
    let current = true;
    call(commands.runGraph(at)).then(
      (next: RunGraph) => current && (n.graph = next),
      () => current && (n.graph = null),
    );
    return () => {
      current = false;
    };
  };

  const shown = $derived.by(() => {
    const graph = n.graph;
    if (!graph) return null;
    const uses = graph.edges.filter((edge) => edge.target === node);
    const usedBy = graph.edges.filter((edge) => edge.source === node);
    const byId = new Map(graph.nodes.map((item) => [item.id, item]));
    const column = (ids: string[], x: number) => {
      const top = -((ids.length - 1) * (NODE.height + GAP.y)) / 2;
      return ids.flatMap((id, at) => {
        const item = byId.get(id);
        return item ? [{ item, x, y: top + at * (NODE.height + GAP.y) }] : [];
      });
    };
    const step = NODE.width + GAP.x;
    const placed = [
      ...column(uses.map((edge) => edge.source), 0),
      ...column([node], step),
      ...column(usedBy.map((edge) => edge.target), 2 * step),
    ];
    const nodes: Node[] = placed.map(({ item, x, y }) => ({
      id: item.id,
      type: "artifact",
      position: { x, y },
      width: NODE.width,
      height: NODE.height,
      data: { name: item.name, light: item.light, open: item.id === node },
      draggable: false,
      connectable: false,
      ariaLabel: `Artifact ${item.name}`,
    }));
    const edges: Edge[] = [...uses, ...usedBy].map((edge) => ({
      id: `${edge.source}>${edge.target}`,
      source: edge.source,
      target: edge.target,
      class: `light-${edge.light}`,
      selectable: false,
      focusable: false,
    }));
    const rows = Math.max(uses.length, usedBy.length, 1);
    return { nodes, edges, height: rows * (NODE.height + GAP.y) + 24, key: placed.map(({ item }) => item.id).join() };
  });
</script>

<div class="host" {@attach reread}>
{#if shown}
  <div class="neighbours" role="group" aria-label="Neighbours" style:height="{Math.min(shown.height, 320)}px">
    {#key shown.key}
      <SvelteFlowProvider>
        <SvelteFlow
          nodes={shown.nodes}
          edges={shown.edges}
          {nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.08, maxZoom: 1 }}
          minZoom={0.2}
          maxZoom={1}
          panOnDrag={false}
          zoomOnScroll={false}
          zoomOnPinch={false}
          zoomOnDoubleClick={false}
          preventScrolling={false}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          proOptions={{ hideAttribution: true }}
          onnodeclick={({ node: clicked }) => clicked.id !== node && onopen(clicked.id)}
        />
      </SvelteFlowProvider>
    {/key}
  </div>
{/if}
</div>

<style>
  .neighbours {
    margin: 0 var(--pad) 12px;
    min-height: 72px;
  }
  .neighbours :global(.svelte-flow) {
    --xy-background-color: transparent;
    --xy-edge-stroke-width: 1.5;
  }
  .neighbours :global(.svelte-flow__edge path) {
    stroke: var(--light, var(--faint));
  }
  .neighbours :global(.svelte-flow__node) {
    cursor: pointer;
  }
</style>
