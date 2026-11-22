<script lang="ts">
  // The star beside an Artifact's Detail title: the star icon stars the Artifact, a favorite of
  // every branch of its project, and the filled star takes the star away. Starred Artifacts are what the graph's find field
  // lists with nothing typed.
  import type { Attachment } from "svelte/attachments";
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";

  let { branch, artifact }: { branch: BranchKey; artifact: string } = $props();
  const { artifactStars } = getAppState();
  const stars = $derived(artifactStars.of(branch));
  const on = $derived(stars.names.includes(artifact));
  const read: Attachment = () => { void artifactStars.load(branch); };
</script>

<button class="star" class:on aria-label="Star {artifact}" aria-pressed={on} {@attach read} onclick={() => artifactStars.toggle(branch, artifact)}><Icon name={on ? "star-on" : "star"} /></button>
{#if stars.error}<span class="error" role="status">{stars.error}</span>{/if}

<style>
  /* Faint until it is on; on, the filled star in ink, as a branch's star is. */
  .star {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-3);
    cursor: pointer;
  }
  .star:hover {
    background: var(--surface-group);
    border-radius: var(--radius-control);
  }
  .star.on {
    color: var(--ink);
  }
  .error {
    min-width: 0;
    color: var(--red);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
</style>
