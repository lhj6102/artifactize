<script lang="ts">
  // The reasons box (D-REV-5, J-HUMAN-REASONS): where Chat's composer stands and drawn as its
  // field, holding only the reasons field and Add reason. Add reason (also Enter) adds what is
  // typed as the newest reason and empties the field; Shift+Enter starts a new line. What is
  // typed and not added is this field's own. Esc leaves the field for the review. In the pet
  // overlay it is one line, with Add and what the overlay puts after it (D-PET-1).
  import type { Snippet } from "svelte";
  import { composing } from "../shell/keys";
  import type { Review } from "./review.svelte";
  import type { ReasonsCaret } from "./reasons-caret.svelte";

  let {
    review,
    typed = $bindable(""),
    caret,
    compact = false,
    onleave,
    after,
  }: {
    review: Review;
    /** What the field holds and has not added. */
    typed?: string;
    caret: ReasonsCaret;
    /** The pet overlay's one-line row. */
    compact?: boolean;
    /** Esc left the field: the review takes the keys again. */
    onleave?: () => void;
    /** What stands after Add in the row (the pet overlay's Point). */
    after?: Snippet;
  } = $props();
  let adding = $state(false);
  const empty = $derived(typed.trim() === "");

  async function add() {
    const text = typed;
    if (text.trim() === "" || adding) return;
    adding = true;
    try {
      // Typed on meanwhile, the field keeps what came after.
      if (await review.addReason(text)) typed = typed.startsWith(text) ? typed.slice(text.length) : typed;
    } finally {
      adding = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (composing(event)) return;
    if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      event.stopPropagation();
      void add();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      (event.currentTarget as HTMLElement).blur();
      onleave?.();
    }
  }
</script>

<div class="box" class:compact>
  {#if compact}
    <input
      class="bare-field"
      aria-label="Reason"
      placeholder="Enter adds the reason"
      data-reasons
      bind:value={typed}
      onkeydown={keydown}
      {@attach caret.attach}
    />
  {:else}
    <textarea
      class="bare-field"
      rows="1"
      aria-label="Reason"
      placeholder="Enter adds the reason"
      data-reasons
      bind:value={typed}
      onkeydown={keydown}
      {@attach caret.attach}
    ></textarea>
  {/if}
  <button class="button {compact ? 'small' : ''}" aria-label={compact ? "Add reason (Enter)" : undefined} aria-keyshortcuts="Enter" disabled={empty || adding} onclick={() => void add()}>{compact ? "Add" : "Add reason"}</button>
  {#if after}{@render after()}{/if}
</div>

<style>
  /* Chat's composer field (D-CHAT-6): the field, then Add reason at its bottom right. */
  .box {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-chip);
    background: var(--surface-input);
  }
  .box:has(textarea:focus) {
    outline: 1px solid var(--current-mark);
    outline-offset: -2px;
  }
  .box:has(textarea:focus-visible) {
    box-shadow: 0 0 0 2px var(--surface-app), 0 0 0 4px var(--focus);
  }
  textarea {
    flex: 1;
    min-width: 0;
    min-height: var(--control);
    max-height: 12em;
    align-self: stretch;
    padding: 4px 0;
    box-sizing: border-box;
    border: 0;
    outline: none;
    resize: none;
    field-sizing: content;
    background: none;
    color: var(--ink);
    font: inherit;
    white-space: pre-wrap;
  }
  textarea::placeholder,
  input::placeholder {
    color: var(--ink-2);
  }
  .button {
    flex: none;
  }
  /* The pet overlay's reasons row: a one-line field taking the row, Add, then Point. */
  .box.compact {
    align-items: center;
    gap: 6px;
    padding: 0;
    border: 0;
    background: none;
  }
  .box.compact input {
    flex: 1;
    min-width: 0;
    height: var(--control);
    padding: 0 7px;
    box-sizing: border-box;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: inherit;
    font-size: var(--text-small);
  }
  .box.compact input:focus {
    outline: 1px solid var(--current-mark);
    outline-offset: -2px;
  }
</style>
