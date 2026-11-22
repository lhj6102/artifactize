<script lang="ts">
  // A branch's live graph, below its scan state in the session header and above its composer.
  import { tick, untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BranchKey, Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { plain } from "../shell/keys";
  import type { GraphWorkspace } from "../monitor/graph-workspace.svelte";
  import { keyOf } from "../monitor/monitor-state.svelte";
  import DetailPane from "../monitor/DetailPane.svelte";
  import GraphStage from "../monitor/GraphStage.svelte";
  import GraphTools from "../monitor/GraphTools.svelte";
  import EditDialogs from "./EditDialogs.svelte";

  let { branch: branchKey, worktree, workspace }: { branch: BranchKey; worktree: string; workspace: GraphWorkspace } = $props();
  const app = getAppState();
  const { current, chat } = app;
  const key = $derived(keyOf(branchKey));
  const branch = $derived.by(() => {
    void key;
    return untrack(() => branchKey);
  });
  const view = $derived(current.of(branch) ? current.view : null);
  const source: Source = $derived({ kind: "current", branch });
  const opened = $derived(workspace.opened && keyOf(workspace.opened.source) === keyOf(source) ? workspace.opened : null);
  const active = $derived(app.mode === "chat" && !app.monitor.settings);

  /** Scan when this Graph view is shown, not on every new session snapshot. */
  const scanning: Attachment = () => {
    void key;
    if (app.mode !== "chat") return;
    untrack(() => {
      if (app.browse.view) current.runsChanged(app.browse.view, branch, false);
      void current.scan(branch);
    });
  };

  function open(node: string, tab: string | null = null) {
    workspace.opened = { source, node, tab };
  }

  function tab(tab: string | null) {
    if (opened && opened.tab !== tab) workspace.opened = { source, node: opened.node, tab };
  }

  function review(id: string) {
    app.review(id);
    if (opened && opened.request !== id) workspace.opened = { ...opened, request: id };
  }

  async function close() {
    if (!opened) return;
    const held = opened.request === undefined ? undefined : app.reviews[opened.request];
    if (held && !held.escape()) return;
    const inside = workspace.panes.detail?.contains(document.activeElement);
    workspace.opened = null;
    if (inside) {
      await tick();
      chat.panes.main?.focus();
    }
  }

  function keydown(event: KeyboardEvent) {
    if (!plain(event, "Escape") || event.defaultPrevented || !opened) return;
    event.preventDefault();
    void close();
  }

  useCommands(() => [
    { id: "main.close", group: "Graph", label: "Close the Detail", keys: "Esc", enabled: () => opened !== null, run: () => void close() },
  ], () => active);
</script>

{#snippet detail()}
  {#if opened && view}
    <DetailPane
      {source}
      revision={view.revision}
      branch={{ key: branch, workspace: worktree }}
      node={opened.node}
      tab={opened.tab}
      focused={workspace.focus === "detail"}
      {active}
      review={(id) => app.reviews[id]}
      openReview={review}
      bind:element={workspace.panes.detail}
      onfocus={() => (workspace.focus = "detail")}
      ontab={tab}
      onopen={open}
      onback={() => void close()}
      ongone={() => (workspace.opened = null)}
    />
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="current" aria-label="Current" {@attach scanning} onkeydown={keydown} onfocusin={(event) => { if (!workspace.panes.detail?.contains(event.target as Node)) workspace.focus = "main"; }}>
  <GraphStage detail={opened && view ? detail : undefined}>
    {#if view}
      {#await import("../monitor/SourceGraph.svelte")}
        <div class="pending"><GraphTools /><p class="note">Loading the graph…</p></div>
      {:then { default: SourceGraph }}
        <SourceGraph {source} {branch} {workspace} {active} revision={view.revision} opened={opened?.node ?? null} onopen={open} onclose={() => void close()} />
      {/await}
    {:else}
      <div class="pending">
        <GraphTools />
        {#if !current.error}<p class="note">Scanning the worktree…</p>{/if}
      </div>
    {/if}
  </GraphStage>
</section>

<EditDialogs branch={active ? branch : null} ondeleted={() => void close()} />

<style>
  .current, .pending {
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* A declared graph keeps its strip while the library reads it. */
  .pending {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
  }
  .note {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
</style>
