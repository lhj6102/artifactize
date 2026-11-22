<script lang="ts">
  // A newer release: the running and the new version as two numbers, then how to update the way
  // this app was installed: the command as selectable text, the file to download, or the
  // release page. The app never updates itself.
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
        <Dialog.Title class="dialog-title">Update artifactize</Dialog.Title>
        <div class="dialog-body">
          <p class="versions" aria-label="Running {running}, available {latest}">{running} → {latest}</p>
          {#if update.kind === "command"}
            <pre class="command">{update.command}</pre>
          {:else if update.kind === "download"}
            <button class="link" onclick={() => void release.follow(latest)}>{update.url}</button>
            {#if update.command}<pre class="command">{update.command}</pre>{/if}
          {:else}
            <button class="link" onclick={() => void release.follow(latest)}>{update.url}</button>
          {/if}
          {#if release.failure}<p class="failure">{release.failure}</p>{/if}
        </div>
      {/if}
      <div class="dialog-actions">
        <Dialog.Close class="button" aria-keyshortcuts="Escape">Close</Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  p {
    margin: 0 0 8px;
  }
  .versions {
    font-variant-numeric: tabular-nums;
  }
  .command {
    margin: 0 0 8px;
    padding: 6px 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    font: var(--text-small) / 1.5 var(--mono);
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
    color: var(--ink-2);
    text-align: left;
    text-decoration: underline;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .link:hover {
    background: var(--surface-group);
    border-radius: var(--radius-control);
  }
  .failure {
    color: var(--red);
  }
</style>
