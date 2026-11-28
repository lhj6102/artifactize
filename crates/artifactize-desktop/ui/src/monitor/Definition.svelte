<script lang="ts">
  // An Artifact's definition on a Current, where a Run's Detail shows its Path, read from the
  // code (D-INS-10): Bound, the file and line of the `artf` statement that binds it; Macro, for
  // one a macro made; Path, as the code resolves it, a missing target said beside it; Parts,
  // each opening that part's Detail; Tools, the toolsets it composes as written. Source follows,
  // folded until unfolded (D-INS-11): the binding's code, read-only and numbered as in its
  // file, and for a macro's Artifact a switch to the declaration the call expands to. Nothing
  // here is a field or a button that edits: the code changes in its file.
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Section from "../shell/Section.svelte";

  let { branch, name, onopen }: { branch: BranchKey; name: string; onopen: (node: string) => void } = $props();
  const app = getAppState();
  const defs = $derived(app.definitionsFor(branch));
  const node = $derived(defs.node(name));

  /** The Source is read once its section is unfolded. */
  const sourceOpen = $derived(app.sections.open("Source"));
  const source = $derived(defs.sources.get(name));
  // The store keeps what was read until the definitions are read again.
  // EFFECT: reading the Source is IPC the section's unfolding asks for, which no derived value may do.
  $effect(() => {
    if (sourceOpen && node) void defs.source(branch, name);
  });

  /** Authored is shown first; Expanded only for an Artifact a macro made. */
  let expanded = $state(false);
  const shown = $derived.by(() => {
    if (!source || "error" in source) return null;
    if (expanded && source.expanded !== null) return { text: source.expanded, first: 1 };
    return { text: source.authored, first: source.line };
  });
  const lines = $derived(shown ? shown.text.replace(/\n$/, "").split("\n") : []);
</script>

{#if node}
  <Section name="Definition">
    <dl class="facts">
      <dt>Bound</dt>
      <dd class="mono quiet">{node.definition}:{node.line}</dd>
      {#if node.macroName}
        <dt>Macro</dt>
        <dd class="mono">{node.macroName}</dd>
      {/if}
      <dt>Path</dt>
      <dd class="path">
        <span class="mono">{node.target}</span>
        {#if node.targetMissing}<span class="missing">{node.target} does not exist.</span>{/if}
      </dd>
      {#if node.parts.length > 0}
        <dt>Parts</dt>
        <dd>
          <ul class="parts" aria-label="Parts of {name}">
            {#each node.parts as part (part)}
              <li><button class="part" onclick={() => onopen(`a:${part}`)}>{part}</button></li>
            {/each}
          </ul>
        </dd>
      {/if}
      {#if node.tools}
        <dt>Tools</dt>
        <dd class="mono">{node.tools}</dd>
      {/if}
    </dl>
  </Section>
  <Section name="Source">
    {#if source && "error" in source}
      <p class="refusal">{source.error}</p>
    {:else if shown}
      {#if source?.expanded !== null}
        <div class="views" role="tablist" aria-label="Source views">
          <button class="view" role="tab" aria-selected={!expanded} onclick={() => (expanded = false)}>Authored</button>
          <span class="bar" aria-hidden="true">|</span>
          <button class="view" role="tab" aria-selected={expanded} onclick={() => (expanded = true)}>Expanded</button>
        </div>
      {/if}
      <div class="source" aria-label="Source of {name}" role="region">
        <ol class="margin" aria-hidden="true">
          {#each lines as _, at (at)}<li>{shown.first + at}</li>{/each}
        </ol>
        <pre class="text">{lines.join("\n")}</pre>
      </div>
    {:else}
      <p class="quiet">Loading…</p>
    {/if}
  </Section>
{:else if defs.error}
  <p class="refusal">{defs.error}</p>
{/if}

<style>
  .facts {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 12px;
    margin: 0;
    padding: 4px 0 0;
    align-items: baseline;
  }
  dt {
    color: var(--ink-2);
    text-align: right;
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .mono {
    font: var(--text-small) var(--mono);
  }
  .quiet {
    color: var(--ink-2);
  }
  .path {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--gap);
  }
  .missing {
    color: var(--red);
    font-size: var(--text-small);
  }
  .parts {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .parts li {
    min-height: var(--row-dense);
    display: flex;
    align-items: center;
  }
  .part {
    margin: 0 -4px;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink);
    font: inherit;
    cursor: pointer;
  }
  .part:hover {
    background: var(--surface-group);
  }
  .part:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .refusal {
    margin: 4px 0 0;
    color: var(--red);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  .views {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 0;
  }
  .view {
    margin: 0 -4px;
    padding: 0 4px 2px;
    border: 0;
    background: none;
    font: inherit;
    color: var(--ink-2);
    cursor: pointer;
  }
  .view:not([aria-selected="true"]):hover {
    color: var(--ink);
    background: var(--surface-group);
  }
  .view[aria-selected="true"] {
    color: var(--ink);
    box-shadow: inset 0 -2px 0 var(--current-mark);
  }
  .view:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .bar {
    color: var(--ink-3);
  }
  /* The code in monospace on a field's surface, its line numbers in faint ink; it scrolls
     sideways in its own box rather than wrap, so each number stays with its line. */
  .source {
    display: flex;
    margin: 4px 0 0;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    font: var(--text-small) / 1.5 var(--mono);
    overflow: hidden;
  }
  .margin {
    flex: none;
    margin: 0;
    padding: 6px 8px 6px 0;
    list-style: none;
    color: var(--ink-2);
    text-align: right;
    min-width: 28px;
    user-select: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 6px 8px 6px 0;
    color: var(--ink);
    font: inherit;
    white-space: pre;
    overflow-x: auto;
    user-select: text;
  }
</style>
