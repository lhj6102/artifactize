<script lang="ts">
  // An Artifact's comment in place under its node, opened by the Definition's Comment or a click
  // on the comment: its text whole, with its line breaks. Enter writes it into the Artifact's
  // file, Shift+Enter breaks the line, Enter on an empty field removes the comment, and Esc or
  // a click elsewhere puts it back. No button and no hint: Help names the keys. What it holds
  // is the Definitions' `fieldText`, set from the comment as it opened.
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";

  let { name, onclose }: { name: string; onclose: (written: string | null) => void } = $props();
  const { definitions: defs } = getAppState();

  /** Focused when it opens, the caret at the end of the text. */
  const focus: Attachment<HTMLTextAreaElement> = (field) => {
    field.focus({ preventScroll: true });
    field.setSelectionRange(field.value.length, field.value.length);
  };

  function keydown(event: KeyboardEvent) {
    if (composing(event)) return;
    if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      event.stopPropagation();
      onclose(defs.fieldText.trim() === "" ? "" : defs.fieldText);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose(null);
    }
  }
</script>

<!-- Typed in, the graph's keys do nothing: the field holds them. -->
<textarea
  class="field nodrag nopan nowheel"
  aria-label="Comment on {name}"
  rows="3"
  bind:value={defs.fieldText}
  onkeydown={keydown}
  onblur={() => onclose(null)}
  onpointerdown={(event) => event.stopPropagation()}
  onclick={(event) => event.stopPropagation()}
  {@attach focus}
></textarea>

<style>
  .field {
    display: block;
    box-sizing: border-box;
    width: 100%;
    min-height: 44px;
    field-sizing: content;
    margin: 0;
    padding: 4px 6px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: var(--text-small) / 1.4 var(--font);
    resize: none;
    outline: 2px solid var(--focus);
    outline-offset: -1px;
    cursor: text;
  }
</style>
