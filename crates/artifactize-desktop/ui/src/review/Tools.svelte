<script lang="ts">
  // The request's Human tools, grouped by the Artifact that declares them (the reviewed eval's
  // own first), and the Tool log of every run in this review. A click runs a tool at once; what
  // it prints arrives in the log line by line while it runs, a launched program's until it
  // exits. The log is plain selectable text: the reviewer copies from it into the result.
  import { untrack } from "svelte";
  import { ToolsState, setToolsState } from "./tools-state.svelte";
  import type { Attachment } from "svelte/attachments";
  import type { ToolView } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import brandMark from "../../../../../assets/brand/artifactize-icon-mark.svg";

  let { review }: { review: Review } = $props();
  const t = new ToolsState();
  setToolsState(t);

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

  /** Rows from the bottom within which the log still counts as followed. */
  const FOLLOW_SLACK = 24;

  /** The log follows its newest line unless the reader scrolled up; scrolled back to the
   * bottom, it follows again. Only the DOM is touched, as lines are added. */
  const follow: Attachment<HTMLElement> = (log) => {
    const scrolled = () => {
      t.following = log.scrollHeight - log.scrollTop - log.clientHeight < FOLLOW_SLACK;
    };
    const added = new MutationObserver(() => {
      if (t.following) log.scrollTop = log.scrollHeight;
    });
    added.observe(log, { childList: true, subtree: true, characterData: true });
    log.addEventListener("scroll", scrolled);
    return () => {
      added.disconnect();
      log.removeEventListener("scroll", scrolled);
    };
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

<section class="logbox" aria-label="Tool log">
  <h4>Tool log</h4>
  <!-- Focusable so the keys scroll it; its text selects and copies like any other. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="log" tabindex="0" role="log" aria-label="Tool log" {@attach follow}>
    {#each review.log as entry (entry.run)}
      <p class="start">▶ {entry.at} · {entry.operation} · {entry.artifact}</p>
      {#each entry.lines as line, at (at)}
        <p class:error={line.error}>{line.text}</p>
      {/each}
      {#if entry.end}<p class="end" class:error={entry.end.error}>{entry.end.text}</p>{/if}
    {:else}
      <p class="muted">What the tools print shows here while this review is open.</p>
    {/each}
  </div>
</section>

<style>
  h4 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .tools,
  .logbox {
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
  .log {
    max-height: 260px;
    min-height: 72px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--surface-2);
    font: 12px/1.5 var(--mono);
    outline: none;
    user-select: text;
  }
  .log:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .log p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
  }
  .start,
  .end {
    color: var(--muted);
  }
  .end {
    margin-bottom: 6px !important;
  }
  .error {
    color: var(--red) !important;
  }
  .muted {
    color: var(--muted);
    margin: 0 0 6px;
    font-family: inherit;
  }
</style>
