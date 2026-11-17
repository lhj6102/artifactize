<script lang="ts">
  // An Artifact's comment on a graph: text alone under its node (or under a whole's title row,
  // `inset`), in the quieter ink, at most three lines; read aloud after its node. On a Current a
  // press on it is the comment's own (`oncomment`): moved, it drags the comment, released, it
  // opens the field in its place (`editing`), which `onfield` closes.
  import CommentField from "./CommentField.svelte";

  let {
    name,
    text,
    editing = false,
    inset = false,
    oncomment,
    onfield,
  }: {
    name: string;
    text: string | null;
    editing?: boolean;
    /** Under a whole's title row, inside its box, rather than under a node. */
    inset?: boolean;
    oncomment?: (event: PointerEvent) => void;
    onfield?: (written: string | null) => void;
  } = $props();
</script>

{#if editing && onfield}
  <div class="place" class:inset><CommentField {name} text={text ?? ""} onclose={onfield} /></div>
{:else if text}
  <!-- On a Current the press is the comment's: it takes no drag or pan of the graph's. The
       keyboard reaches a comment through the Definition's Comment button. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <p class="place comment" class:inset class:nodrag={oncomment !== undefined} class:nopan={oncomment !== undefined} onpointerdown={oncomment} onclick={(event) => oncomment && event.stopPropagation()}>{text}</p>
{/if}

<style>
  .place {
    position: absolute;
    left: 0;
    top: calc(100% + 4px);
    width: 168px;
    margin: 0;
  }
  .place.inset {
    position: static;
    width: auto;
    padding: 4px 10px 0;
  }
  .comment {
    color: var(--ink-2);
    font-size: var(--text-small);
    font-weight: 400;
    line-height: 1.4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    cursor: inherit;
  }
</style>
