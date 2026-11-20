<script lang="ts">
  // A waiting Human eval's review, under its row in an Artifact's Evals on a Run's graph
  // (J-DETAIL-REVIEW, D-INS-12): Claim to review while it can be claimed, Release claim while it
  // is this reviewer's, then Open in Review. The claim is the same as Review's: one Review per
  // request, kept by the window.
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, type Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Reference from "../shell/Reference.svelte";

  let { source, node, revision }: { source: Source; node: string; revision: number } = $props();
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

<div class="review" {@attach read}>
  {#if review && view && view.stage !== "completed"}
    <!-- Its stage: claimable, this reviewer's, or taken by its claimant (J-DETAIL-REVIEW). -->
    <span class="stage" class:yours={review.owned}>{review.owned ? "yours" : view.claim ? `taken by ${view.claim.reviewer}` : "claimable"}</span>
    {#if review.owned}
      <button class="button small" disabled={review.busy !== null} onclick={() => void review.release()}>Release claim</button>
    {:else if review.claimable}
      <button class="button small" disabled={review.busy !== null} onclick={() => void review.claim()}>Claim to review</button>
    {/if}
    <Reference onclick={() => request && void app.showReview(request)}>Open in Review</Reference>
    {#if review.notice?.error}<span class="error" role="status">{review.notice.text}</span>{/if}
  {/if}
</div>

<style>
  /* Under the eval's row, indented to its name. */
  .review:not(:empty) {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-height: var(--control);
    padding: 2px var(--pad) 6px calc(var(--pad) + var(--icon) + 8px);
    font-size: var(--text-small);
  }
  .stage {
    color: var(--ink-2);
  }
  .stage.yours {
    color: var(--accent);
  }
  .error {
    color: var(--red);
    overflow-wrap: anywhere;
  }
</style>
