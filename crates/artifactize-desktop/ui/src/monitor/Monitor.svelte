<script lang="ts">
  // Scope and Runs on the left, the selected Run's graph filling the right. Focus moves between
  // the panes; a node's summary floats beside it, and its Detail floats over the graph's right.
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BrowseView } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { MonitorState, setMonitorState, type Focus } from "./monitor-state.svelte";
  import { find } from "./tree.svelte";
  import ScopePane from "./ScopePane.svelte";
  import RunsPane from "./RunsPane.svelte";
  import RunPane from "./RunPane.svelte";
  import DetailPane from "./DetailPane.svelte";
  import Summary from "./Summary.svelte";
  import { Review } from "../review/review.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";

  let { active }: { active: boolean } = $props();
  const app = getAppState();
  const { browse } = app;

  const m = new MonitorState();
  setMonitorState(m);
  const { tree, panes } = m;

  const view = $derived(browse.view);
  const run = $derived(view?.run ?? null);

  /** What a snapshot changes here, as it is accepted. */
  function accept(shown: BrowseView) {
    const at = shown.run;
    if (at) tree.sync(at.id, at.tree);
    // A replaced state: Details, drafts and sessions of the old one no longer apply.
    if (shown.replaced) {
      m.opened = null;
      app.forgetReviews();
      if (m.focus === "detail") m.focus = "graph";
    }
    // Leaving the selected Run (or losing it) closes its Detail.
    if (m.opened !== null && m.opened.run !== at?.id) m.opened = null;
    // Another Run's graph starts without a popup or a node under the pointer.
    const id = at?.id ?? null;
    if (id !== m.popupRun) {
      m.popupRun = id;
      m.popup = null;
      m.hovered = null;
    }
  }

  onMount(() => {
    if (browse.view) accept(browse.view);
    return browse.onShown(accept);
  });

  /** The Review of a request, if one was opened: rendering only looks. */
  const review = (id: string): Review | undefined => app.reviews[id];
  /** Open the Review of a request when its Detail arrives, never while rendering. */
  function openReview(id: string): void {
    app.review(id);
  }

  /** Move focus to a pane's list or review, or else to the pane itself. The graph takes it on
   * its popup or selected Artifact, as soon as it is drawn. */
  async function move(to: Focus) {
    m.focus = to;
    await tick();
    if (to === "graph") {
      if (m.graphFocus?.()) return;
      m.graphPending = true;
      panes.graph?.focus();
      return;
    }
    const pane = panes[to];
    (pane?.querySelector<HTMLElement>('[tabindex="0"]') ?? pane)?.focus();
  }

  /** A graph was drawn (or went away): it takes the focus asked for while none could. */
  function ready(focus: (() => boolean) | null) {
    m.graphFocus = focus;
    if (!focus || !m.graphPending || !active || m.focus !== "graph") return;
    // Focus that moved on meanwhile stays where it went.
    if (!panes.graph?.contains(document.activeElement)) return;
    m.graphPending = false;
    focus();
  }

  /** Show the selection in the graph: an eval in its Artifact's popup, an Artifact by itself. */
  function follow() {
    m.popup = tree.selected.length > 1 ? tree.selected[0] : null;
    void move("graph");
  }

  function openDetail() {
    const node = tree.selected.at(-1);
    if (!run || !node) return;
    m.opened = { run: run.id, node };
    void move("detail");
  }

  function openRun() {
    const row = run?.tree.find((node) => node.kind === "run");
    if (!run || !row) return;
    m.opened = { run: run.id, node: row.id };
    void move("detail");
  }

  function leaveDetail() {
    m.opened = null;
    void move("graph");
  }

  /** `!`: the next ERROR, RED or waiting Human eval in its Artifact's popup; from Detail, its
   * Detail. */
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
    m.popup = tree.selected[0];
    if (fromDetail) openDetail();
    else void move("graph");
  }

  /** `b`: the upstream Artifact the selection waits for. */
  function blocker() {
    if (run && tree.jumpBlocker(run.tree)) follow();
  }

  /** Backspace: back to where the `b` jumps started. */
  function jumpBack() {
    if (tree.jumpBack()) follow();
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

  /** The node the summary describes: the one under the pointer, else the graph's selection
   * while the graph has the keyboard. An Artifact whose evals pop up shows them instead. */
  const summarized = $derived.by(() => {
    const keys = m.focus === "graph" ? (m.popup ? tree.selected : tree.selected.slice(0, 1)) : null;
    const path = m.hovered ?? keys;
    if (!run || !path?.length || (path.length === 1 && path[0] === m.popup)) return null;
    // The Run itself has no node in the graph: its headline opens its Detail.
    const node = find(run.tree, path);
    return node && node.kind !== "run" ? path : null;
  });
  /** Where the summary floats: beside the eval's popup row, or beside the Artifact's node. */
  const anchor = $derived(
    summarized === null
      ? ""
      : summarized.length > 1
        ? `.evals [data-path="${CSS.escape(summarized.join("/"))}"]`
        : `.svelte-flow__node[data-id="${CSS.escape(summarized[0])}"]`,
  );

  const upstream = $derived(run ? (find(run.tree, tree.origin())?.upstream.length ?? 0) : 0);

  useCommands(() =>
    active
      ? [
          { id: "monitor.refresh", group: "Monitor", label: "Refresh", keys: "Ctrl+R", run: () => browse.refresh() },
          { id: "monitor.attention", group: "Monitor", label: "Next attention", keys: "!", run: attention },
          { id: "monitor.scope", group: "Monitor", label: "Focus Scope", run: () => void move("scope") },
          { id: "monitor.runs", group: "Monitor", label: "Focus Runs", run: () => void move("runs") },
          { id: "monitor.graph", group: "Monitor", label: "Focus Run graph", run: () => void move("graph") },
          {
            id: "monitor.open",
            group: "Monitor",
            label: "Open Detail of the selected node",
            keys: "o",
            enabled: () => run !== null && tree.selected.length > 0,
            run: openDetail,
          },
          { id: "monitor.run", group: "Monitor", label: "Open the Run's Detail", enabled: () => run !== null, run: openRun },
          {
            id: "monitor.blocker",
            group: "Monitor",
            label: "Go to the upstream blocker",
            keys: "b",
            enabled: () => upstream > 0,
            run: blocker,
          },
          {
            id: "monitor.back",
            group: "Monitor",
            label: "Back to where the blocker jumps started",
            keys: "Backspace",
            enabled: () => tree.blocker !== null,
            run: jumpBack,
          },
        ]
      : [],
  );
</script>

<svelte:window onkeydown={keydown} />

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
          {tree}
          notice={m.notice ?? browse.error ?? view?.error ?? null}
          bind:popup={m.popup}
          {active}
          focused={m.focus === "graph"}
          bind:element={panes.graph}
          onfocus={() => (m.focus = "graph")}
          onopen={openDetail}
          onrun={openRun}
          onback={() => void move("runs")}
          onblocker={blocker}
          onjumpback={jumpBack}
          onready={ready}
          onhover={(path) => (m.hovered = path)}
        />
        {#if run && summarized}
          <Summary
            run={run.id}
            node={summarized[summarized.length - 1]}
            {anchor}
            hint={summarized.length > 1 ? "Enter or click: Detail" : "Enter or click: evals · o: Detail"}
          />
        {/if}
        {#if m.opened}
          <aside class="overlay" aria-label="Detail">
            <DetailPane
              run={m.opened.run}
              node={m.opened.node}
              focused={m.focus === "detail"}
              {active}
              {review}
              {openReview}
              bind:element={panes.detail}
              onfocus={() => (m.focus = "detail")}
              onback={leaveDetail}
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
  }
  :global(.columns) {
    min-height: 0;
  }
  .stage {
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .overlay {
    position: absolute;
    z-index: 20;
    top: 8px;
    right: 8px;
    bottom: 8px;
    width: min(46%, 680px);
    min-width: min(340px, calc(100% - 16px));
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgb(0 0 0 / 22%);
  }
</style>
