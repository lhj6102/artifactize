<script lang="ts">
  // The decision bar (D-REV-12, J-HUMAN-CLAIM, J-HUMAN-GREEN, J-HUMAN-RED): under the issues box,
  // apart from it. At its left the claim: where it stands, Claim to review or Release claim, and a
  // running claim's or release's seconds and Cancel; at its right the decision: GREEN and RED as
  // tabs, why Submit cannot submit yet, and Submit naming the chosen verdict, its seconds and
  // Cancel in its place while it runs. Too narrow for both, the decision drops under the claim.
  // A review taken by another reviewer says only who holds it. The pet overlay has no claim, and
  // says why Submit waits on its own line above the row (D-PET-1).
  import type { Attachment } from "svelte/attachments";
  import type { Verdict } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import { label } from "../shell/keys";
  import type { Review } from "./review.svelte";

  let { review, typed, compact = false, onsubmit }: {
    review: Review;
    /** What the issues field holds and has not added. */
    typed: string;
    /** The pet overlay's decision row: no claim, why above the row. */
    compact?: boolean;
    onsubmit: () => void;
  } = $props();
  const verdicts = ["GREEN", "RED"] as const;
  const key = (verdict: Verdict) => (verdict === "GREEN" ? "g" : "r");

  const view = $derived(review.view);
  const busy = $derived(review.busy);
  /** The clock of the running work's seconds; it ticks only while they are shown (the pet
   * overlay has no app clock). */
  let now = $state(Date.now());
  const ticking: Attachment = () => {
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  };
  const seconds = $derived(busy ? Math.max(0, Math.floor((now - busy.since) / 1000)) : 0);
  const claiming = $derived(busy?.action === "claim" || busy?.action === "release");
  const submitting = $derived(busy?.action === "submit");
  /** Choosing waits only for a claim, a release or a submission; a tool's run does not hold it. */
  const choosable = $derived(review.owned && (busy === null || busy.tool));
  const stage = $derived(review.owned ? "Yours" : "Waiting");
  /** Why Submit waits: the library's refusal first, then what the draft still needs. */
  const why = $derived(review.owned && !submitting ? review.refusal ?? review.blocked(typed) : null);
  const refused = $derived(review.owned && review.refusal !== null);
</script>

{#snippet progress()}
  <span class="busy" role="status" {@attach ticking}>
    <span class="tone-running"><Icon name="running" label={busy?.label ?? ""} /></span>
    <span class="seconds">{seconds}s</span>
    <button class="button small quiet" aria-label="Cancel (Esc)" aria-keyshortcuts="Escape" onclick={() => review.cancel()}>Cancel</button>
  </span>
{/snippet}

{#snippet decide()}
  <span class="decide">
    <span class="tabs" role="radiogroup" aria-label="Verdict">
      {#each verdicts as verdict (verdict)}
        <button
          class="tab {verdict.toLowerCase()}"
          role="radio"
          aria-checked={review.verdict === verdict}
          aria-label="{verdict} ({key(verdict)})"
          aria-keyshortcuts={key(verdict)}
          disabled={!choosable}
          onclick={() => void review.choose(verdict)}
        >{verdict}</button>
      {/each}
    </span>
    {#if why && !compact}<span class="why" class:failed={refused} role="status">{why}</span>{/if}
    {#if submitting}
      {@render progress()}
    {:else}
      <button
        class="button primary submit"
        aria-label="Submit{review.verdict ? ` ${review.verdict}` : ''} ({label('Ctrl+S')})"
        aria-keyshortcuts="Control+S"
        disabled={!review.submittable(typed)}
        onclick={onsubmit}
      >Submit{review.verdict ? ` ${review.verdict}` : ""}</button>
    {/if}
  </span>
{/snippet}

{#if view && !review.settled}
  {#if compact}
    {#if review.owned}
      {#if why}<p class="why own" class:failed={refused} role="status">{why}</p>{/if}
      <div class="bar compact" role="group" aria-label="Decision">{@render decide()}</div>
    {/if}
  {:else if !review.owned && view.claim}
    <div class="bar" role="group" aria-label="Decision"><span class="said">Taken by {view.claim.reviewer}</span></div>
  {:else}
    <div class="bar" role="group" aria-label="Decision">
      <span class="claim">
        <span class="stage">{stage}</span>
        {#if review.owned}
          <button class="button small quiet" class:held={busy !== null && !claiming} aria-label="Release claim (u)" aria-keyshortcuts="u" disabled={busy !== null} onclick={() => void review.release()}>Release claim</button>
        {:else}
          <button class="button primary" aria-label="Claim to review (c)" aria-keyshortcuts="c" disabled={!review.claimable || busy !== null} onclick={() => void review.claim()}>Claim to review</button>
        {/if}
        {#if claiming}
          {@render progress()}
        {:else if review.notice?.error}
          <span class="failed" role="status">{review.notice.text}</span>
        {/if}
      </span>
      {@render decide()}
    </div>
  {/if}
{/if}

<style>
  /* A bar of its own on the panel surface, 8px under the issues box (D-REV-12). */
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px var(--gap);
    min-height: 40px;
    margin-top: 8px;
    padding: 4px 8px;
    box-sizing: border-box;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    background: var(--surface-panel);
  }
  .claim,
  .decide {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-width: 0;
  }
  .decide {
    margin-left: auto;
  }
  .stage,
  .said,
  .why {
    min-width: 0;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .stage {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .why {
    white-space: nowrap;
  }
  .failed {
    min-width: 0;
    color: var(--red);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  .why.failed {
    white-space: normal;
  }
  /* Release claim held while other work of the review runs. */
  .held:disabled {
    color: var(--ink-3);
  }
  /* GREEN and RED drawn as tabs are (D-SYS-13): a dot in the verdict's colour, the chosen one
     bright over the current mark, neither marked until one is chosen. */
  .tabs {
    display: inline-flex;
    gap: 16px;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--control);
    padding: 0 4px;
    border: 0;
    background: none;
    color: var(--ink-off);
    font: inherit;
    font-size: var(--text-small);
    font-weight: 400;
    cursor: pointer;
  }
  .tab::before {
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .tab.green::before {
    background: var(--green);
  }
  .tab.red::before {
    background: var(--red);
  }
  .tab:hover:not(:disabled):not([aria-checked="true"]) {
    color: var(--ink);
  }
  .tab[aria-checked="true"] {
    color: var(--ink-bright);
    box-shadow: inset 0 -2px 0 var(--current-mark);
  }
  .tab:disabled {
    color: var(--ink-3);
    cursor: default;
  }
  .tab:disabled::before {
    opacity: 0.5;
  }
  .submit {
    flex: none;
  }
  .busy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .tone-running {
    display: inline-flex;
  }
  .seconds {
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }
  /* The pet overlay's decision row: the review's decision bar without the claim, the choice at
     the left and Submit at the right. */
  .bar.compact {
    margin-top: 0;
  }
  .bar.compact .decide {
    flex: 1;
    justify-content: space-between;
    margin-left: 0;
  }
  .why.own {
    margin: 0;
    white-space: normal;
  }
</style>
