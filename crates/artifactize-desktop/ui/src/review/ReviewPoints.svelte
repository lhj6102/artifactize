<script lang="ts">
  import type { Attachment } from "svelte/attachments";
  import type { ReviewPoint } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import type { Review } from "./review.svelte";

  let { review, insert, ready, compact = false, limited = false }: {
    review: Review;
    insert: (text: string) => void;
    /** A reasons field is there to take an Insert. */
    ready: boolean;
    compact?: boolean;
    limited?: boolean;
  } = $props();

  const points = $derived(review.draft?.points ?? []);
  const key = (point: ReviewPoint) => JSON.stringify([point.at, point.source, point.text]);
  const newest = $derived(review.latestPoint ? key(review.latestPoint) : null);
  const time = (at: string) => new Date(at).toTimeString().slice(0, 5);
  /** Keep a newly pointed row in view, without scrolling on edits to the result form. */
  const follow: Attachment<HTMLUListElement> = (list) => {
    if (!(compact || limited) || newest === null) return;
    queueMicrotask(() => (list.scrollTop = list.scrollHeight));
  };
</script>

<ul class="points" class:compact class:limited aria-label="Points" {@attach follow}>
  {#each points as point, at (`${key(point)}:${at}`)}
    <li class:newest={at === points.length - 1 && key(point) === newest}>
      <span class="point"><Icon name="point" /></span>
      <span class="text">{point.text}</span>
      {#if !compact}
        <span class="source"><span class="source-name">{point.source}</span><span>{` · ${time(point.at)}`}</span></span>
      {/if}
      <button class="button small" disabled={!ready} onclick={() => insert(point.text)}>Insert</button>
      {#if !compact}
        <button class="remove" aria-label="Remove point: {point.text}" onclick={() => void review.removePoint(point)}><Icon name="remove" /></button>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .points {
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }
  .points.limited { max-height: calc(5 * var(--row)); }
  .points.compact { flex: none; max-height: calc(3 * var(--row)); }
  li {
    display: grid;
    grid-template-columns: auto minmax(2ch, 1fr) minmax(0, 0.8fr) auto auto;
    align-items: center;
    gap: 6px;
    height: var(--row);
    font-size: var(--text-small);
  }
  .compact li { grid-template-columns: auto minmax(0, 1fr) auto; }
  li.newest { background: var(--accent-soft); }
  .point { display: inline-flex; color: var(--ink-3); }
  .text,
  .source {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text { font-family: var(--mono); }
  .source { display: flex; color: var(--ink-2); font-variant-numeric: tabular-nums; }
  .source-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .source > span:last-child { flex: none; }
  .remove {
    display: inline-grid;
    place-items: center;
    width: var(--row-dense);
    height: var(--row-dense);
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .remove:hover { color: var(--ink); background: var(--surface-group); }
</style>
