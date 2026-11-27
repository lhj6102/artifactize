<script lang="ts">
  // The reviewer's issues (D-REV-10, J-HUMAN-REASONS): oldest first, each drawn as a person's
  // message is in Chat, its text exactly as typed, Edit and Remove beside it at its left. Edit
  // turns the bubble into a field in its place: Enter keeps the change, Esc leaves it as it was,
  // Shift+Enter starts a new line. In the pet overlay each is one line with Remove alone (D-PET-1).
  import type { Attachment } from "svelte/attachments";
  import { composing } from "../shell/keys";
  import Icon from "../shell/Icon.svelte";
  import type { Review } from "./review.svelte";

  let { review, compact = false, onleave }: {
    review: Review;
    compact?: boolean;
    /** An edit ended by the keyboard: the review takes the keys again. */
    onleave?: () => void;
  } = $props();

  /** The issue being edited: where it stood, what it read when the edit began, what it reads now. */
  let editing = $state<{ index: number; previous: string; text: string } | null>(null);
  const issues = $derived(review.issues);
  const usable = $derived(review.owned && !review.settled);

  /** The edit's field takes the caret at its end as it opens. */
  const takeCaret: Attachment<HTMLTextAreaElement> = (field) => {
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  };

  async function keep() {
    const edit = editing;
    if (!edit) return;
    editing = null;
    await review.editIssue(edit.index, edit.previous, edit.text);
  }

  function keydown(event: KeyboardEvent) {
    if (composing(event)) return;
    if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      event.stopPropagation();
      void keep().then(() => onleave?.());
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      editing = null;
      onleave?.();
    }
  }
</script>

{#if issues.length > 0}
  <ol class="issues" class:compact aria-label="Issues">
    {#each issues as issue, index (`${index}:${issue}`)}
      <li class="issue">
        {#if editing?.index === index}
          <textarea
            class="editing"
            aria-label="Edit issue {index + 1}"
            bind:value={editing.text}
            onkeydown={keydown}
            onblur={() => void keep()}
            {@attach takeCaret}
          ></textarea>
        {:else}
          <span class="tools">
            {#if !compact}
              <button class="icon" aria-label="Edit issue {index + 1}" title="Edit" disabled={!usable} onclick={() => (editing = { index, previous: issue, text: issue })}><Icon name="edit" /></button>
            {/if}
            <button class="icon" aria-label="Remove issue {index + 1}" title="Remove" disabled={!usable} onclick={() => void review.removeIssue(index, issue)}><Icon name="remove" /></button>
          </span>
          <p class="message">{issue}</p>
        {/if}
      </li>
    {/each}
  </ol>
{/if}

<style>
  .issues {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 6px 0 0;
    list-style: none;
  }
  .issue {
    align-self: flex-end;
    display: flex;
    align-items: flex-start;
    gap: 4px;
    max-width: 80%;
    min-width: 0;
  }
  .issue:has(.editing) {
    align-self: stretch;
    max-width: none;
    justify-content: flex-end;
  }
  /* A person's message in Chat (D-CHAT-3): text exactly as typed, never Markdown. */
  .message {
    min-width: 0;
    margin: 0;
    padding: 6px 10px;
    border-radius: var(--radius-chip);
    background: var(--surface-group);
    color: var(--ink);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .editing {
    flex: 1;
    max-width: 80%;
    min-height: var(--control);
    padding: 6px 10px;
    box-sizing: border-box;
    border: 0;
    border-radius: var(--radius-chip);
    outline: none;
    box-shadow: inset 0 0 0 1px var(--current-mark);
    resize: none;
    field-sizing: content;
    background: var(--surface-input);
    color: var(--ink);
    font: inherit;
  }
  .tools {
    flex: none;
    display: flex;
    padding-top: 5px;
  }
  .icon {
    display: inline-flex;
    padding: 0 3px;
    border: 0;
    background: none;
    color: var(--ink-3);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    color: var(--ink);
  }
  .icon:disabled {
    cursor: default;
  }
  /* The pet overlay: one line each, at most three rows tall, scrolling past that. */
  .compact {
    max-height: calc(3 * var(--row) + 16px);
    overflow: auto;
    padding: 0;
    gap: 4px;
  }
  .compact .issue {
    max-width: 100%;
  }
  .compact .tools {
    padding-top: 3px;
  }
  .compact .message {
    padding: 2px 8px;
    font-size: var(--text-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
