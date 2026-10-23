<script lang="ts">
  // A newer release: the version and how to update the way this app was installed. The app
  // never updates itself.
  import { Dialog } from "bits-ui";
  import type { Release } from "./release.svelte";

  let { release }: { release: Release } = $props();
</script>

<Dialog.Root bind:open={release.open}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog">
      {#if release.notice}
        {@const { running, latest, update } = release.notice}
        <Dialog.Title class="dialog-title">artifactize {latest} is available</Dialog.Title>
        <p>This app is {running}.</p>
        {#if update.kind === "command"}
          <p>To update, run:</p>
          <pre class="command">{update.command}</pre>
        {:else if update.kind === "download"}
          <p>To update, download and install it the way you installed this one:</p>
          <button class="link" onclick={() => void release.follow()}>{update.url}</button>
          {#if update.command}
            <p>then run, in the folder you downloaded it to:</p>
            <pre class="command">{update.command}</pre>
          {/if}
        {:else}
          <p>See the release page for how to update:</p>
          <button class="link" onclick={() => void release.follow()}>{update.url}</button>
        {/if}
        {#if release.failure}<p class="failure">{release.failure}</p>{/if}
      {/if}
      <div class="dialog-actions">
        <Dialog.Close class="button">Close</Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  p {
    margin: 0 0 8px;
  }
  .command {
    margin: 0 0 8px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface-2);
    font-family: var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .link {
    display: block;
    margin: 0 0 8px;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: var(--accent);
    text-align: left;
    text-decoration: underline;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .failure {
    color: var(--red);
  }
</style>
