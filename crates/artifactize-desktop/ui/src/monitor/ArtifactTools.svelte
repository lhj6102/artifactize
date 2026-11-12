<script lang="ts">
  // An Artifact's own Human tools in its branch's worktree as it is now, whichever Run is shown,
  // each a ▶ button, and their Tool log. A click runs the tool at once, with no review and no
  // claim; nothing it does is recorded as a review's evidence.
  import { onMount } from "svelte";
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import ToolLog from "../review/ToolLog.svelte";

  let {
    branch,
    workspace,
    artifact,
  }: {
    branch: BranchKey;
    /** The branch's worktree as the backend names it: the tools' log is kept by it. */
    workspace: string;
    artifact: string;
  } = $props();
  // The Detail keys this component by worktree and Artifact: its tools stay one owner's. Each
  // call names the branch checked out in that worktree now.
  // svelte-ignore state_referenced_locally
  const tools = getAppState().artifactTools(workspace, artifact);

  onMount(() => void tools.load(branch));

  const declared = $derived(tools.tools ?? []);
</script>

<section class="tools" aria-label="Tools of {tools.artifact}">
  <h4>Tools</h4>
  {#if tools.error}
    <p class="error">{tools.error}</p>
  {:else if tools.tools === null}
    <p class="muted">Loading…</p>
  {:else if declared.length === 0}
    <p class="muted">{tools.artifact} declares no Human tools.</p>
  {:else}
    {#each declared as tool (tool.name)}
      <div class="tool">
        <button class="button small" disabled={tools.busy !== null} onclick={() => void tools.run(branch, tool)}>▶ {tool.operation}</button>
        <span class="description">{tool.description}</span>
        {#if tool.error}<span class="reason">{tool.error}</span>{/if}
      </div>
    {/each}
  {/if}
  {#if tools.busy}
    <p class="muted" role="status">{tools.busy.label}… <button class="button small" onclick={() => tools.cancel()}>Cancel</button></p>
  {:else if tools.notice}
    <p class="notice" class:error={tools.notice.error} role="status">{tools.notice.text}</p>
  {/if}
</section>

<div class="log">
  <ToolLog entries={tools.log} empty="What its tools print shows here while the app runs." />
</div>

<style>
  /* Each section after the neighbours and evals is opened above by a group line. */
  .tools,
  .log {
    min-width: 0;
    margin: 0 var(--pad);
    padding: 8px 0 10px;
    border-top: 1px solid var(--line-group);
  }
  /* A section label: 11px uppercase in secondary ink. */
  h4 {
    margin: 0 0 4px;
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .tool {
    display: grid;
    grid-template-columns: minmax(0, max-content) minmax(0, 1fr);
    align-items: baseline;
    gap: 2px 10px;
    padding: 2px 0;
  }
  /* A long operation name wraps inside its button rather than widening the Detail. */
  .tool .button {
    height: auto;
    min-height: 22px;
    max-width: 100%;
    white-space: normal;
    text-align: left;
    overflow-wrap: anywhere;
  }
  .description,
  .reason {
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  .description,
  .muted {
    color: var(--ink-2);
  }
  .reason,
  .error {
    color: var(--red);
  }
  .reason {
    grid-column: 2;
  }
  .notice,
  .muted,
  .error {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
  }
</style>
