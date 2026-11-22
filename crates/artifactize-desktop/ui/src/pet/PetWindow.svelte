<script lang="ts">
  // The pet overlay (J-PET, D-PET-1): a small frameless window above every window while this
  // reviewer works away from the active review's evaluation space. It holds the review's eval and
  // Run (a link to the review in Review), the same library draft's Points, the GREEN and RED
  // choice and the open form, Point (pointing in an open viewer of the review's Artifact) and
  // Submit. The main window shows and hides it; it follows the active review as it changes.
  import { emit, listen } from "@tauri-apps/api/event";
  import { onMount, untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { commands, onActiveReviewChanged, onHumanDraftChanged, onStateChanged, subscribe } from "../ipc/client";
  import { Jobs } from "../ipc/jobs.svelte";
  import { Review } from "../review/review.svelte";
  import { FormCaret } from "../review/form-caret";
  import ReviewPoints from "../review/ReviewPoints.svelte";
  import VerdictChoices from "../review/VerdictChoices.svelte";
  import VerdictForm from "../review/VerdictForm.svelte";
  import Icon from "../shell/Icon.svelte";
  import { ARM, ASK, PRESENCE, type Presence } from "../viewers/viewer-state.svelte";
  import { PET_OPEN } from "./pet";
  import petImage from "../shell/pet.svg";

  let { request }: { request: string } = $props();
  const jobs = new Jobs();
  const caret = new FormCaret();
  /** The review the overlay acts on: the active one. */
  let review = $state(new Review(untrack(() => request), jobs));
  const view = $derived(review.view);
  /** The viewers open, by session, as they announce themselves. */
  const viewers = new SvelteMap<number, Presence>();
  /** The open viewer Point arms: the one the library matches to this review's original request. */
  const pointer = $derived(view === null ? null : ([...viewers.values()].find((each) => each.open && each.pointable && each.petMatch && each.review === review.id) ?? null));
  const run = $derived(view?.target.run ?? "");
  const evalId = $derived(view?.target.node.replace(/^e:/, "") ?? "");

  /** Read review `id`; a verdict its draft holds comes back as that verdict's open form. */
  async function show(id: string) {
    if (review.id !== id) review = new Review(id, jobs);
    await review.load();
  }

  async function submit() {
    await review.submit();
  }

  function open() {
    void emit(PET_OPEN, review.id);
    void commands.mainFocus();
  }

  onMount(() => {
    void show(request);
    const undrafted = onHumanDraftChanged((id) => {
      if (id === review.view?.owner) void review.readDraft();
    });
    const unstate = onStateChanged((resync) => {
      if (resync && review.owned) void review.readDraft();
    });
    const unactive = onActiveReviewChanged((next) => {
      if (next) void show(next);
    });
    const unpresent = subscribe<Presence>((handler) => listen(PRESENCE, handler), (presence) => {
      if (presence.open) viewers.set(presence.session, presence);
      else viewers.delete(presence.session);
    });
    void emit(ASK);
    return () => {
      unstate();
      undrafted();
      unactive();
      unpresent();
    };
  });
</script>

<div class="pet-window">
  <div class="card">
    <button class="head" onclick={open} aria-label="Show {evalId} in Review">
      <img class="pet" src={petImage} alt="" width="40" height="40" />
      <span class="tone-human"><Icon name="human" /></span><span class="eval">{evalId}</span><span class="run">· {run}</span>
    </button>
    {#if view && review.owned}
      <div class="scroll">
        {#if review.draft?.points.length}
          <ReviewPoints {review} compact insert={(text) => caret.insert(text)} />
        {/if}
        <!-- The verdict's form carries the GREEN / RED choice once open, as the review's does. -->
        {#if review.form}
          {#key review.form}<VerdictForm {review} form={review.form} {caret} showError={false} />{/key}
        {:else}
          <VerdictChoices {review} />
        {/if}
      </div>
      {#if review.form?.error}<p class="error" role="alert">{review.form.error}</p>{:else if review.notice?.error}<p class="error" role="status">{review.notice.text}</p>{/if}
      <div class="row">
        <button class="button small point" aria-pressed={pointer?.pointing ?? false} disabled={pointer === null} onclick={() => pointer && void emit(ARM, { session: pointer.session })}><Icon name="point" />Point</button>
        {#if pointer === null}<span class="why">Open {view.artifact} in a viewer to point.</span>{/if}
        <span class="spacer"></span>
        {#if review.form}
          <button class="button small primary" disabled={!review.submittable} onclick={() => void submit()}>Submit {review.form.verdict}</button>
        {/if}
      </div>
    {:else if view}
      <p class="why">{review.settled ? "Submitted." : "This review is no longer claimed here."}</p>
    {/if}
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    background: transparent;
  }
  .pet-window {
    position: relative;
    height: 100%;
    padding-top: 20px;
    box-sizing: border-box;
  }
  .pet {
    position: absolute;
    top: 0;
    left: 12px;
    z-index: 1;
    filter: saturate(60%);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: calc(100% - 4px);
    padding: 12px;

    box-sizing: border-box;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-chip);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin: 0 0 0 46px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink);
    font: var(--text-small) var(--mono);
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--surface-group);
  }
  .tone-human {
    display: inline-flex;
    color: var(--human);
  }
  .eval,
  .run {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .run {
    color: var(--ink-2);
  }
  .scroll {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 280px;
    overflow: auto;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  .point { color: var(--ink-2); }
  .point[aria-pressed="true"] { color: var(--ink-bright); }
  .point:disabled {
    background: none;
    border-color: var(--line-section);
    color: var(--ink-3);
  }
  .spacer {
    flex: 1;
  }
  .why {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .error {
    margin: 0;
    color: var(--red);
    font-size: var(--text-small);
  }
</style>
