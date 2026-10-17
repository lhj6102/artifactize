<script lang="ts">
  // The laid-out graph and, beside the Artifact it was opened on, a popup of that Artifact's
  // evals with their verdicts. Nodes are focusable: arrows move between connected Artifacts,
  // Enter or a click pops up the evals, Esc closes them. An eval selected in the popup is the
  // Run's selection; Enter or a click on it asks for its Detail.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { SvelteFlow, Controls, MiniMap, useSvelteFlow, getViewportForBounds, type Edge, type Node } from "@xyflow/svelte";
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { RunGraph, TreeNode } from "../ipc/client";
  import ArtifactNode from "./ArtifactNode.svelte";
  import { LEGEND, WORDS, neighbor, type Arrow, type Point } from "./graph";
  import { NODE } from "./layout";
  import { beside } from "./float";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    graph,
    positions,
    tree,
    selected,
    popup = $bindable(null),
    active = true,
    structural = false,
    onselect,
    onopen,
    onready,
    onhover,
  }: {
    graph: Pick<RunGraph, "nodes" | "edges">;
    /** Static structure has no Run verdicts. */
    structural?: boolean;
    positions: Map<string, Point>;
    /** The Run's tree rows: an Artifact's evals come from there. */
    tree: TreeNode[];
    selected: string[];
    /** The Artifact whose evals pop up beside it. */
    popup?: string | null;
    /** The graph's screen is shown: only then may it hold keyboard focus. */
    active?: boolean;
    onselect: (path: string[]) => void;
    onopen: () => void;
    /** Offers, once the canvas and its focus handlers are mounted, a way to give the graph
     * keyboard focus (on the popup, else on the selected Artifact), which says whether it
     * could; `null` once the canvas is gone. */
    onready?: (focus: (() => boolean) | null) => void;
    /** The Artifact (`[artifact]`) or popup eval (`[artifact, eval]`) under the pointer. */
    onhover?: (path: string[] | null) => void;
  } = $props();
  const v = new GraphViewState();
  setGraphViewState(v);

  const flow = useSvelteFlow();
  const nodeTypes = { artifact: ArtifactNode };
  /** The camera and its controls share the same zoom limits. */
  const MIN_ZOOM = 0.05;
  const MAX_ZOOM = 2;
  // Lights change in place; positions only come with a new layout. The selection is not part
  // of a node: a node whose selection flips is drawn again and loses focus, so focus (which
  // follows the selection) marks the selected Artifact instead.
  let nodes = $derived.by<Node[]>(() =>
    graph.nodes
      .filter((node) => positions.has(node.id))
      .map((node) => ({
        id: node.id,
        type: "artifact",
        position: positions.get(node.id)!,
        // A fixed size: recoloured nodes keep their dimensions and never re-measure.
        width: NODE.width,
        height: NODE.height,
        data: { name: node.name, light: node.light, open: node.id === popup, structural },
        draggable: false,
        connectable: false,
        ariaLabel: structural ? `${node.name}: current structure` : `${node.name}: ${WORDS[node.light]}`,
        domAttributes: { "data-light": node.light },
      })),
  );
  let edges = $derived.by<Edge[]>(() =>
    graph.edges.map((edge) => ({
      id: `${edge.source}>${edge.target}`,
      source: edge.source,
      target: edge.target,
      class: `light-${edge.light}`,
      selectable: false,
      focusable: false,
      domAttributes: { "data-light": edge.light },
    })),
  );

  const artifact = $derived(popup ? (tree.find((node) => node.id === popup) ?? null) : null);
  const evals = $derived(artifact?.children ?? []);

  /** The eval of the popup's Artifact the list points at: the selection moves the highlight;
   * when the selection leaves the evals, the highlight stays and Enter selects it again. */
  const index = $derived.by(() => {
    const at = popup && selected[0] === popup ? evals.findIndex((node) => node.id === selected[1]) : -1;
    return at >= 0 ? at : Math.min(v.picked, Math.max(evals.length - 1, 0));
  });

  /** Open the Detail of the highlighted eval, which becomes the selection first. */
  function openEval() {
    if (!popup || !evals[index]) return;
    onselect([popup, evals[index].id]);
    onopen();
  }

  function element(id: string): HTMLElement | null {
    return v.canvas?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(id)}"]`) ?? null;
  }

  /** The node is drawn and wholly inside the canvas. */
  function inView(id: string): boolean {
    const box = element(id)?.getBoundingClientRect();
    const view = v.canvas?.getBoundingClientRect();
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
  // reaching the graph. A redraw that removes the focused node is caught as it happens (a
  // mutation observer runs before the next key event), and a key that still lands on the page
  // while the visible graph holds focus is handled for the held node. Each handoff is
  // numbered; a newer one voids older ones. Focus that
  // moves anywhere else (the eval list, another pane, a press outside) ends the hold, and so
  // does hiding the graph's screen: a hidden graph never takes focus, and only fresh graph
  // focus holds again.

  const lost = () => document.activeElement === null || document.activeElement === document.body;

  function watch() {
    if (v.watching) return;
    v.watching = true;
    const step = () => {
      if (v.held !== null && hidden()) release();
      if (v.held === null || !v.canvas?.isConnected) {
        v.watching = false;
        return;
      }
      restore();
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  /** Put focus back on the held node, or on the canvas while it is not drawn. */
  function restore() {
    if (v.held === null || !v.canvas || hidden()) return;
    const target = element(v.held);
    const active = document.activeElement;
    if (target && (lost() || active === v.canvas)) target.focus({ preventScroll: true });
    else if (!target && lost()) v.canvas.focus({ preventScroll: true });
  }

  /** The graph's screen is not shown (another screen, or a hidden ancestor). */
  const hidden = () => !active || !v.canvas || v.canvas.closest("[hidden]") !== null;

  /** Give graph focus to `id` from now on; returns this v.handoff's number. */
  function hold(id: string): number {
    if (hidden()) return v.handoff;
    v.held = id;
    v.handoff += 1;
    if (document.activeElement !== element(id)) element(id)?.focus({ preventScroll: true });
    watch();
    return v.handoff;
  }

  function release() {
    v.held = null;
    v.handoff += 1;
  }

  /** Focus or a press anywhere outside the graph's nodes and canvas ends the hold; so does
   * leaving the screen, which the watch notices on its next frame. */
  const holdFocus: Attachment<HTMLElement> = (canvas) => {
    const focusin = (event: FocusEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target || target === canvas) return;
      const id = canvas.contains(target) ? target.closest<HTMLElement>(".svelte-flow__node")?.dataset.id : undefined;
      if (!id) return release();
      if (id !== v.held) hold(id);
      // Focus is the graph's selection, so the keys always start from the node in focus; another
      // Artifact in focus closes the popup.
      if (id === popup) return;
      popup = null;
      if (id !== selected[0]) onselect([id]);
    };
    const pointerdown = (event: PointerEvent) => {
      if (!canvas.contains(event.target as HTMLElement | null)) release();
    };
    // Keys that land on the page during a redraw belong to the held node of the shown graph.
    const pagekey = (event: KeyboardEvent) => {
      if (v.held === null || hidden()) return;
      if (event.target !== document.body && event.target !== document.documentElement) return;
      const id = v.held;
      restore();
      handle(event, id);
    };
    // A redraw that removes the focused node: focus goes back before any key can arrive.
    const redrawn = new MutationObserver(() => restore());
    redrawn.observe(canvas, { childList: true, subtree: true });
    document.addEventListener("focusin", focusin);
    document.addEventListener("pointerdown", pointerdown, true);
    document.addEventListener("keydown", pagekey);
    return () => {
      document.removeEventListener("focusin", focusin);
      document.removeEventListener("pointerdown", pointerdown, true);
      document.removeEventListener("keydown", pagekey);
      redrawn.disconnect();
      release();
    };
  };

  /** The node under the pointer, told as the pointer moves anywhere on the page: an Artifact,
   * an eval of the popup, or nothing. Movement is what every pointer reports, real or driven. */
  const pointer: Attachment<HTMLElement> = (root) => {
    let last = "";
    const tell = (path: string[] | null) => {
      const key = path?.join("\n") ?? "";
      if (key === last) return;
      last = key;
      onhover?.(path);
    };
    const move = (event: MouseEvent) => {
      const target = event.target instanceof Element && root.contains(event.target) ? event.target : null;
      const row = target?.closest<HTMLElement>("[data-eval]");
      const node = target?.closest<HTMLElement>(".svelte-flow__node")?.dataset.id;
      tell(row?.dataset.eval && popup ? [popup, row.dataset.eval] : node ? [node] : null);
    };
    const left = () => tell(null);
    document.addEventListener("mousemove", move);
    document.documentElement.addEventListener("mouseleave", left);
    return () => {
      document.removeEventListener("mousemove", move);
      document.documentElement.removeEventListener("mouseleave", left);
      tell(null);
    };
  };

  /** A mounted view offers its caller a way to give it focus; the caller decides when. */
  const ready: Attachment<HTMLElement> = (canvas) => {
    void tick().then(() => {
      if (!canvas.isConnected) return;
      onready?.(() => {
        if (!canvas.isConnected || hidden()) return false;
        if (popup && v.list) {
          void centre(popup);
          v.list.focus({ preventScroll: true });
          return true;
        }
        const id = positions.has(selected[0]) ? selected[0] : graph.nodes[0]?.id;
        if (!id) return false;
        hold(id);
        void centre(id);
        return true;
      });
    });
    return () => onready?.(null);
  };

  /** Expanding the structural text changes the canvas height. Keep the whole graph in view
   * rather than clipping it below the new canvas; the Run view keeps its own camera. An open
   * popup never narrows the camera to its Artifact: every node stays a target. */
  const structuralSize: Attachment<HTMLElement> = (canvas) => {
    if (!structural) return;
    let previous = "";
    const resized = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width === 0 || height === 0) return;
      const size = `${width}:${height}`;
      if (previous && previous !== size) {
        void fit({ duration: 0, maxZoom: 1.6, padding: 0.6 });
      }
      previous = size;
    });
    resized.observe(canvas);
    return () => resized.disconnect();
  };

  /** fitView shares a pending resolver across calls. Use the public bounds calculation and
   * a distinct viewport transition, so an old fit cannot finish a newer one early. */
  function fit(options: { id?: string; duration: number; maxZoom?: number; padding?: number }) {
    if (!v.canvas) return Promise.resolve(false);
    const bounds = flow.getNodesBounds(options.id ? [options.id] : flow.getNodes());
    const viewport = getViewportForBounds(
      bounds, v.canvas.clientWidth, v.canvas.clientHeight,
      MIN_ZOOM, options.maxZoom ?? MAX_ZOOM, options.padding ?? 0.1,
    );
    return flow.setViewport(viewport, { duration: options.duration });
  }

  /** Pop up the evals of `id` beside it, the selected one (else the first) highlighted and
   * the list focused. */
  async function openEvals(id: string) {
    release();
    popup = id;
    const at = selected[0] === id ? evals.findIndex((node) => node.id === selected[1]) : -1;
    v.picked = Math.max(at, 0);
    onselect(evals[v.picked] ? [id, evals[v.picked].id] : [id]);
    await tick();
    if (popup !== id || hidden()) return;
    v.list?.focus({ preventScroll: true });
  }

  /** Close the popup, keyboard focus back on its Artifact. */
  function closeEvals() {
    const id = popup;
    if (!id) return;
    // Hold the node first, so the list leaving takes no focus with it.
    hold(id);
    popup = null;
    onselect([id]);
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

  /** Keys on the canvas: arrows move along edges, Enter pops up the evals, Esc closes them. */
  function keydown(event: KeyboardEvent) {
    // On a node, or on the canvas while the held node is not drawn.
    const focused = nodeOf(event) ?? (event.target === v.canvas ? (v.held ?? undefined) : undefined);
    if (focused) handle(event, focused);
  }

  /** A key for the Artifact node `focused`. */
  function handle(event: KeyboardEvent, focused: string) {
    // From the selected Artifact, which focus follows; the focused node if they differ.
    const id = selected[0] && positions.has(selected[0]) ? selected[0] : focused;
    const arrows: Arrow[] = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    const arrow = arrows.find((key) => plain(event, key));
    if (arrow) {
      const next = neighbor(graph, positions, id, arrow);
      if (next) {
        popup = null;
        void reveal(next);
      }
    } else if (plain(event, "Enter")) void openEvals(focused);
    else if (plain(event, "Escape") && popup) closeEvals();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function listKey(event: KeyboardEvent) {
    const move = (to: number) => {
      v.picked = Math.min(Math.max(to, 0), evals.length - 1);
      if (popup && evals[v.picked]) onselect([popup, evals[v.picked].id]);
    };
    if (plain(event, "ArrowDown") || plain(event, "j")) move(index + 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) move(index - 1);
    else if ((plain(event, "Enter") || plain(event, "o") || plain(event, "ArrowRight")) && evals[index]) openEval();
    else if (plain(event, "Escape") || plain(event, "ArrowLeft")) closeEvals();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<div class="graph" {@attach pointer}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="canvas" bind:this={v.canvas} {@attach holdFocus} {@attach ready} {@attach structuralSize} onkeydown={keydown} tabindex="-1">
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView
      minZoom={MIN_ZOOM}
      maxZoom={MAX_ZOOM}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={true}
      autoPanOnNodeFocus={false}
      elevateNodesOnSelect={false}
      onlyRenderVisibleElements={graph.nodes.length > 80}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => void openEvals(node.id)}
      onpaneclick={() => popup && closeEvals()}
    >
      <Controls showLock={false} />
      {#if graph.nodes.length > 40}<MiniMap pannable zoomable />{/if}
    </SvelteFlow>
    <ul class="legend" aria-label="Legend">
      {#if structural}
        <li>Current structure · arrows: input → consumer</li>
      {:else}
      {#each LEGEND as light (light)}
        <li class="light-{light}"><span class="swatch"></span>{WORDS[light]}</li>
      {/each}
      {/if}
    </ul>
  </div>
  {#if artifact}
    <section class="evals" aria-label="Evals of {artifact.name}" {@attach beside(`.svelte-flow__node[data-id="${CSS.escape(artifact.id)}"]`)}>
      <header>
        <strong>{artifact.name}</strong>
        <span class="muted">{artifact.right}</span>
        <button class="button small" onclick={closeEvals} title="Close the evals">Close <kbd>Esc</kbd></button>
      </header>
      <ul role="listbox" aria-label="Evals" tabindex="0" bind:this={v.list} onkeydown={listKey}>
        {#each evals as node, at (node.id)}
          <li
            role="option"
            aria-selected={at === index}
            class:selected={at === index}
            class="weight-{node.weight}"
            data-path="{artifact.id}/{node.id}"
            data-eval={node.id}
            onclick={() => {
              v.picked = at;
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
    position: relative;
    height: 100%;
    display: grid;
    grid-template-rows: 1fr;
    min-height: 0;
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
    position: absolute;
    z-index: 10;
    width: min(360px, calc(100% - 16px));
    max-height: 60%;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 24px rgb(0 0 0 / 18%);
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
