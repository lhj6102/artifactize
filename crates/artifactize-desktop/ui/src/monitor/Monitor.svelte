<script lang="ts">
  // Scope and Runs on the left, the selected Run's graph on the right, always with the divider
  // between them. Focus moves between the panes with the keys; a click on an Artifact or on the
  // Run's headline opens its Detail, floating over the graph's right side. Dragging the Detail's
  // left edge sets its width, kept like the split ratio.
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BrowseView } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { MonitorState, setMonitorState, type Focus } from "./monitor-state.svelte";
  import { GAP, clampWidth, saveWidth } from "./detail-width";
  import ScopePane from "./ScopePane.svelte";
  import RunsPane from "./RunsPane.svelte";
  import RunPane from "./RunPane.svelte";
  import DetailPane from "./DetailPane.svelte";
  import { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";

  let { active }: { active: boolean } = $props();
  const app = getAppState();
  const { browse } = app;

  const m = new MonitorState();
  setMonitorState(m);
  const { panes } = m;

  const view = $derived(browse.view);
  const run = $derived(view?.run ?? null);

  /** What a snapshot changes here, as it is accepted. */
  function accept(shown: BrowseView) {
    const at = shown.run;
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (shown.replaced) {
      m.opened = null;
      app.forgetReviews();
      if (m.focus === "detail") m.focus = "graph";
    }
    // Leaving the selected Run (or losing it) closes its Detail; so does losing the node, which
    // the Detail finds as it reads its row.
    if (m.opened !== null && m.opened.run !== at?.id) m.opened = null;
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });

  /** The Review of a request, if one was opened: rendering only looks. */
  const review = (id: string): Review | undefined => app.reviews[id];
  /** Open the Review of a request when its Detail arrives, never while rendering; the open
   * Detail notes it, so leaving the Detail asks that review first. */
  function openReview(id: string): void {
    app.review(id);
    if (m.opened && m.opened.request !== id) m.opened = { ...m.opened, request: id };
  }

  /** Move focus to a pane's list or review, or else to the pane itself; the graph's pane takes
   * it itself, and Tab goes on to its Artifacts. */
  async function move(to: Focus) {
    m.focus = to;
    await tick();
    const pane = panes[to];
    if (to === "graph") pane?.focus();
    else (pane?.querySelector<HTMLElement>('[tabindex="0"]') ?? pane)?.focus();
  }

  /** A click on an Artifact: its Detail, on the Artifact's own tab. */
  function openNode(node: string) {
    if (!run) return;
    m.opened = { run: run.id, node, tab: null };
  }

  /** A click on the Run's headline: the Run's Detail. */
  function openRun() {
    if (run) m.opened = { run: run.id, node: run.node, tab: null };
  }

  /** Another tab of the open Detail; a Human review's tab notes its request when it arrives. */
  function showTab(tab: string | null) {
    if (m.opened && m.opened.tab !== tab) m.opened = { run: m.opened.run, node: m.opened.node, tab };
  }

  /** Close the Detail; focus that was in it goes back to the graph. Leaving one step at a time:
   * a Human review shown in it first cancels its running work, then leaves its form, and only
   * then the Detail closes. */
  function closeDetail() {
    if (m.opened === null) return;
    const held = m.opened.request === undefined ? undefined : app.reviews[m.opened.request];
    if (held && !held.escape()) return;
    const inside = panes.detail?.contains(document.activeElement) ?? false;
    m.opened = null;
    if (inside) void move("graph");
  }

  /** Esc in the graph closes its Detail, else steps back to the Runs. */
  function back() {
    if (m.opened) closeDetail();
    else void move("runs");
  }

  /** Dragging the Detail's left edge: the mouse moves it wherever it goes until the button is
   * released, as the pane dividers follow it. */
  function grab(event: MouseEvent) {
    if (event.button !== 0) return;
    const stage = (event.currentTarget as HTMLElement).closest(".stage");
    if (!stage) return;
    event.preventDefault();
    const drag = (moved: MouseEvent) => {
      const room = stage.getBoundingClientRect();
      m.detailWidth = clampWidth(room.right - GAP - moved.clientX, room.width);
    };
    const drop = () => {
      window.removeEventListener("mousemove", drag);
      window.removeEventListener("mouseup", drop);
      if (m.detailWidth !== null) saveWidth(m.detailWidth);
    };
    window.addEventListener("mousemove", drag);
    window.addEventListener("mouseup", drop);
  }

  /** The Runs pane has focus first, as in the terminal monitor. */
  const begin: Attachment = () => {
    if (active && !m.started && view?.refreshed) {
      m.started = true;
      void move(m.focus);
    }
  };

  useCommands(() =>
    active
      ? [
          { id: "monitor.refresh", group: "Monitor", label: "Refresh", keys: "Ctrl+R", run: () => browse.refresh() },
          { id: "monitor.scope", group: "Monitor", label: "Focus Scope", run: () => void move("scope") },
          { id: "monitor.runs", group: "Monitor", label: "Focus Runs", run: () => void move("runs") },
          { id: "monitor.graph", group: "Monitor", label: "Focus Run graph", run: () => void move("graph") },
          { id: "monitor.run", group: "Monitor", label: "Open the Run's Detail", enabled: () => run !== null, run: openRun },
          {
            id: "monitor.close",
            group: "Monitor",
            label: "Close the Detail",
            keys: "Esc",
            enabled: () => m.opened !== null,
            run: closeDetail,
          },
        ]
      : [],
  );
</script>

<div class="monitor" {@attach begin}>
  <PaneGroup direction="horizontal" autoSaveId="monitor-layout" class="columns">
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
            onnext={() => void move("graph")}
            onback={() => void move("scope")}
          />
        </Pane>
      </PaneGroup>
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={76} minSize={30}>
      <div class="stage">
        <RunPane
          notice={browse.error ?? view?.error ?? null}
          bind:noticeOpen={m.noticeOpen}
          opened={m.opened && m.opened.run === run?.id ? m.opened.node : null}
          cover={m.dock}
          focused={m.focus === "graph"}
          bind:element={panes.graph}
          onfocus={() => (m.focus = "graph")}
          onopen={openNode}
          onclose={closeDetail}
          onrun={openRun}
          onback={back}
        />
        {#if m.opened && run && m.opened.run === run.id}
          <aside class="dock" aria-label="Detail" bind:this={m.dock} style:--detail-width={m.detailWidth === null ? null : `${m.detailWidth}px`}>
            <!-- The left edge takes a mouse drag, as the pane dividers do; hovering it shows only
                 the resize cursor. -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div class="edge" role="separator" aria-orientation="vertical" aria-label="Detail width" onmousedown={grab}></div>
            <DetailPane
              run={m.opened.run}
              node={m.opened.node}
              tab={m.opened.tab}
              focused={m.focus === "detail"}
              {active}
              {review}
              {openReview}
              bind:element={panes.detail}
              onfocus={() => (m.focus = "detail")}
              ontab={showTab}
              onopen={openNode}
              onback={closeDetail}
              ongone={() => (m.opened = null)}
            />
          </aside>
        {/if}
      </div>
    </Pane>
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
  /* The graph's own layers (canvas, Detail) stack inside the stage, so none of them covers the
   * divider's grab zone on the graph's side, and the Detail never sits beside Scope or Runs. */
  .stage {
    position: relative;
    isolation: isolate;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* Docked toward the graph's right edge with a gap; the graph stays visible behind it. The
   * Detail sets its own width, by default or by a drag, within the graph's area (as
   * detail-width.ts fits a drag); its content never sizes it, and wraps or scrolls in its own
   * boxes. */
  .dock {
    position: absolute;
    z-index: 20;
    top: 12px;
    right: 12px;
    bottom: 12px;
    width: var(--detail-width, min(46%, 680px));
    min-width: min(320px, calc(100% - 24px));
    max-width: calc(100% - 108px);
    contain: inline-size;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgb(0 0 0 / 22%);
  }
  .edge {
    position: absolute;
    z-index: 1;
    top: 0;
    bottom: 0;
    left: 0;
    width: 6px;
    cursor: col-resize;
    touch-action: none;
  }
</style>
