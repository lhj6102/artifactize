<script lang="ts">
  // A review's Notes (J-HUMAN-NOTES, D-REV-4): a text field whose lines Point appended stand in
  // --mono on --accent-soft until the Notes are next edited. The field stays one plain text area
  // holding the draft's text; while lines are pointed, a mirror of that text drawn behind it shows
  // them, the area's own text then drawn transparent over its mirror, caret and selection kept.
  import { pointedLines } from "./pointed";

  let { value, pointed, oninput, rows = 3 }: { value: string; pointed: readonly string[]; oninput: (text: string) => void; rows?: number } = $props();

  const BREAK = "\n";
  /** A last empty line still takes its height in the mirror, as it does in the area. */
  const END = "​";
  const lines = $derived(pointedLines(value, pointed));
  const mirrored = $derived(lines.some((line) => line.pointed));
  let backdrop = $state<HTMLElement>();

  /** The mirror scrolls with the area it stands behind. */
  function scrolled(event: Event) {
    const area = event.currentTarget as HTMLTextAreaElement;
    if (backdrop) backdrop.scrollTop = area.scrollTop;
  }
</script>

<span class="field" class:mirrored>
  {#if mirrored}
    <span class="backdrop" aria-hidden="true" bind:this={backdrop}>{#each lines as line, at (at)}{#if line.pointed}<mark>{line.text}</mark>{:else}{line.text}{/if}{at < lines.length - 1 ? BREAK : ""}{/each}{END}</span>
  {/if}
  <!-- Named outright: the mirror inside the field's label is not part of its name. -->
  <textarea {rows} aria-label="Notes" placeholder="Optional" {value} oninput={(event) => oninput(event.currentTarget.value)} onscroll={scrolled}></textarea>
</span>

<style>
  .field {
    display: grid;
    min-width: 0;
  }
  .backdrop,
  textarea {
    grid-area: 1 / 1;
    box-sizing: border-box;
    min-height: 56px;
    margin: 0;
    padding: 6px 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    font: inherit;
    line-height: 1.4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  textarea {
    resize: vertical;
    background: var(--surface-input);
    color: var(--ink);
  }
  textarea::placeholder {
    color: var(--ink-3);
  }
  .backdrop {
    overflow: hidden;
    background: var(--surface-input);
    color: var(--ink);
    pointer-events: none;
  }
  /* The mirror shows the text: the area over it keeps its caret and selection. */
  .mirrored textarea {
    background: transparent;
    color: transparent;
    caret-color: var(--ink);
  }
  mark {
    border-radius: 2px;
    background: var(--accent-soft);
    color: var(--ink);
    font-family: var(--mono);
  }
</style>
