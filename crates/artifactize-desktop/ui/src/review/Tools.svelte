<script lang="ts">
  // The request's Human tools, grouped by the Artifact that declares them (the reviewed eval's
  // own first), and the Tool log of every run in this review. A click runs a tool at once; what
  // it prints arrives in the log line by line while it runs, a launched program's until it
  // exits. The log is plain selectable text: the reviewer copies from it into the result.
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { ToolView } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import ToolLog from "./ToolLog.svelte";
  import brandMark from "../../../../../assets/brand/artifactize-icon-mark.svg";

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

<section class="tools" aria-label="Tools" tabindex="-1" onfocusin={() => (review.area = "tools")} {@attach focusTools}>
  <h4>Tools</h4>
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
        <p class="artifact"><img class="mark" src={brandMark} alt="" />{group.artifact}</p>
        {#each group.tools as tool (tool.name)}
          <div class="tool">
            <button class="button small" disabled={!usable} onclick={() => run(tool)}>▶ {tool.operation}</button>
            <span class="description">{tool.description}</span>
            {#if tool.error}<span class="reason">{tool.error}</span>{/if}
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</section>

<ToolLog entries={review.log} empty="What the tools print shows here while this review is open." />

<style>
  h4 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .tools {
    min-width: 0;
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
    width: 1.1em;
    height: 1.1em;
  }
  .tool {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: baseline;
    gap: 2px 10px;
    padding: 2px 0 2px 18px;
  }
  .description,
  .reason {
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .description {
    color: var(--muted);
  }
  .reason {
    grid-column: 2;
    color: var(--red);
  }
  .muted {
    color: var(--muted);
    margin: 0 0 6px;
  }
</style>
