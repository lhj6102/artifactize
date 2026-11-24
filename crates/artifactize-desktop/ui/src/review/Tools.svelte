<script lang="ts">
  // The request's Human tools, grouped by the Artifact that declares them (the reviewed eval's
  // own first, a group led by its Artifact only when more than one declares tools), each a small
  // button led by the run icon with its description. A click runs a tool at once; what it
  // prints arrives in the review's Tool log line by line while it runs.
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { ToolView } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import Icon from "../shell/Icon.svelte";

  let { review }: { review: Review } = $props();

  const tools = $derived(review.view?.tools ?? []);
  /** Tools by Artifact, the reviewed eval's Artifact first, then by name. */
  const groups = $derived.by(() => {
    const own = review.view?.artifact ?? "";
    const names = [...new Set(tools.map((tool) => tool.artifact))].sort(
      (a, b) => Number(b === own) - Number(a === own) || a.localeCompare(b),
    );
    return names.map((artifact) => ({ artifact, tools: tools.filter((tool) => tool.artifact === artifact) }));
  });
  const usable = $derived(review.owned && !review.busy && !review.settled);

  function run(tool: ToolView) {
    void review.run(tools.indexOf(tool));
  }

  /** A review opened with Tools focused gives them the keyboard once, as they appear; Tab
   * then reaches each tool. Later focus moves change the area without taking focus back. */
  const focusTools: Attachment<HTMLElement> = (section) => {
    if (untrack(() => review.area) === "tools") section.focus();
  };
</script>

<div class="tools" role="group" aria-label="Tools" tabindex="-1" onfocusin={() => (review.area = "tools")} {@attach focusTools}>
  {#if tools.length === 0}
    <p class="muted">
      {review.view?.recorded
        ? "No Human tools are declared in this eval's scope."
        : "Unknown: no Human definition is recorded."}
    </p>
  {:else}
    <!-- One row of run buttons per group, each named by its operation, its description its
         tooltip (D-REV-2). -->
    {#each groups as group (group.artifact)}
      <div class="group" role="group" aria-label="Tools of {group.artifact}">
        {#if groups.length > 1}<p class="artifact">{group.artifact}</p>{/if}
        <div class="row">
          {#each group.tools as tool (tool.name)}
            {@const claimant = review.view?.claim && !review.owned ? review.view.claim.reviewer : null}
            {@const hint = review.owned || review.settled ? tool.description : claimant ? `Taken by ${claimant}` : "Claim to run the tools"}
            <button class="button small" title={hint} disabled={!usable} onclick={() => run(tool)}><Icon name="play" />{tool.operation}</button>
          {/each}
        </div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .tools {
    min-width: 0;
    outline: none;
  }
  .group {
    margin-bottom: 8px;
  }
  .artifact {
    margin: 0 0 4px;
    color: var(--ink-2);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  /* A long operation name wraps inside its button rather than widening the Detail. */
  .row .button {
    height: auto;
    min-height: var(--row-dense);
    max-width: 100%;
    white-space: normal;
    text-align: left;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--ink-2);
    margin: 0 0 6px;
  }
</style>
