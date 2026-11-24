<script lang="ts">
  // An Artifact's own Human tools in its branch's worktree as it is now, whichever Run is shown,
  // each a small button led by the run icon, then, from the first run on, their Tool log; no
  // Tools section for an Artifact that declares none. A click
  // runs the tool at once, with no review and no claim, and unfolds a Tool log that was folded;
  // nothing it does is recorded as a review's evidence. What ended well is said by the log's end
  // line; only a failure is said here.
  import { onMount } from "svelte";
  import type { BranchKey, ToolView } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import Section from "../shell/Section.svelte";
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
  const app = getAppState();
  // The Detail keys this component by worktree, branch and Artifact, the owner of these tools.
  // svelte-ignore state_referenced_locally
  const tools = app.artifactTools(workspace, branch, artifact);

  onMount(() => void tools.load(branch));

  const declared = $derived(tools.tools ?? []);

  function run(tool: ToolView) {
    app.sections.set("Tool log", true);
    void tools.run(branch, tool);
  }
</script>

<!-- A section with nothing to hold is not shown: an Artifact without Human tools has none. -->
{#if tools.error || declared.length > 0}
<Section name="Tools" label="Tools of {tools.artifact}">
  {#if tools.error}
    <p class="error">{tools.error}</p>
  {:else}
    {#each declared as tool (tool.name)}
      <div class="tool">
        <button class="button small" disabled={tools.busy !== null} onclick={() => run(tool)}><Icon name="play" />{tool.operation}</button>
        <span class="description">{tool.description}</span>
        {#if tool.error}<span class="reason">{tool.error}</span>{/if}
      </div>
    {/each}
  {/if}
  {#if tools.busy}
    <p class="busy" role="status"><span class="running"><Icon name="running" label={tools.busy.label} /></span><button class="button small" onclick={() => tools.cancel()}>Cancel</button></p>
  {:else if tools.notice?.error}
    <p class="error" role="status">{tools.notice.text}</p>
  {/if}
</Section>
{/if}
{#if tools.log.length > 0}
  <Section name="Tool log">
    <ToolLog entries={tools.log} />
  </Section>
{/if}

<style>
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
    min-height: var(--row-dense);
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
  .description {
    color: var(--ink-2);
  }
  .reason,
  .error {
    color: var(--red);
  }
  .reason {
    grid-column: 2;
  }
  .busy {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  .running {
    display: inline-flex;
    color: var(--running);
  }
  .busy,
  .error {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
  }
</style>
