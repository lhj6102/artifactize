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
  import brandMark from "@brand/artifactize-icon-mark.svg";

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
    {#if !review.owned && !review.settled}<p class="muted">Claim the review to use its tools.</p>{/if}
    {#each groups as group (group.artifact)}
      <div class="group" role="group" aria-label="Tools of {group.artifact}">
        {#if groups.length > 1}<p class="artifact"><img class="mark" src={brandMark} alt="" />{group.artifact}</p>{/if}
        {#each group.tools as tool (tool.name)}
          <div class="tool" class:nested={groups.length > 1}>
            <button class="button small" disabled={!usable} onclick={() => run(tool)}><Icon name="play" />{tool.operation}</button>
            <span class="description">{tool.description}</span>
            {#if tool.error}<span class="reason">{tool.error}</span>{/if}
          </div>
        {/each}
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
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0 0 4px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .mark {
    flex: none;
    width: var(--icon);
    height: var(--icon);
  }
  .tool {
    display: grid;
    grid-template-columns: minmax(0, max-content) minmax(0, 1fr);
    align-items: baseline;
    gap: 2px 10px;
    padding: 2px 0;
  }
  .tool.nested {
    padding-left: 18px;
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
  .description {
    color: var(--ink-2);
  }
  .reason {
    grid-column: 2;
    color: var(--red);
  }
  .muted {
    color: var(--ink-2);
    margin: 0 0 6px;
  }
</style>
