<script lang="ts">
  // A waiting Human eval's review, under its row in an Artifact's Evals on a Run's graph
  // (J-DETAIL-REVIEW, D-INS-12): Claim to review while it can be claimed, Release claim while it
  // is this reviewer's, then Open in Review. The claim is the same as Review's: one Review per
  // request, kept by the window.
  import type { Snippet } from "svelte";
  import ReviewProgress from "../review/ReviewProgress.svelte";
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, type Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { plain } from "../shell/keys";
  import Reference from "../shell/Reference.svelte";

  let { source, node, revision, row }: { source: Source; node: string; revision: number; row: Snippet<[Snippet?]> } = $props();
  const app = getAppState();
  const ipc = useIpc();
  /** The eval's request, once its Detail named it. */
  let request = $state<string | null>(null);
  const review = $derived(request === null ? undefined : app.reviews[request]);
  const view = $derived(review?.view ?? null);

  /** The eval's request, read for each new snapshot; its review is the window's one. */
  const read: Attachment = () => {
    void revision;
    let current = true;
    call(ipc.detail(source, node)).then(
      (detail) => {
        if (!current || detail.profile !== "human" || detail.request === null) return;
        const id = detail.request;
        void app.review(id).load().then(() => {
          if (current) request = id;
        });
      },
      () => {},
    );
    return () => {
      current = false;
    };
  };
</script>

{#snippet stage()}
  {#if review && view && !review.settled}
    <span class="stage" class:yours={review.owned}>· {review.owned ? "yours" : view.claim ? `taken by ${view.claim.reviewer}` : "claimable"}</span>
  {/if}
{/snippet}

<div {@attach read}>
  {@render row(stage)}
  {#if review && view && !review.settled}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="review" onkeydown={(event) => {
      if (!review?.busy || !plain(event, "Escape")) return;
      event.preventDefault();
      event.stopPropagation();
      review.cancel();
    }}>
        {#if review.owned}
          <button class="button small" disabled={review.busy !== null} onclick={() => void review.release()}>Release claim</button>
        {:else if review.claimable}
          <button class="button small" disabled={review.busy !== null} onclick={() => void review.claim()}>Claim to review</button>
        {/if}
        <Reference onclick={() => request && void app.showReview(request)}>Open in Review</Reference>
        <ReviewProgress {review} />
    </div>
  {/if}
</div>

<style>
  /* Under the eval's row, indented to its name. */
  .review {
    display: flex;
    align-items: center;
    gap: var(--gap);
    flex-wrap: wrap;
    min-height: var(--control);
    padding: 2px var(--pad) 6px calc(var(--pad) + var(--icon) + 8px);
    font-size: var(--text-small);
  }
  .stage {
    color: var(--ink-2);
  }
  .stage.yours {
    color: var(--ink);
  }
</style>
