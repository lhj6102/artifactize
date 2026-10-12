<script lang="ts">
  // Quitting with claims the standalone review took and did not submit: keep or release them.
  import { AlertDialog } from "bits-ui";
  import { commands, call, message, nextJob } from "../ipc/client";

  let {
    taken,
    onstay,
    onleave,
  }: { taken: string[] | null; onstay: () => void; onleave: () => void } = $props();

  let working = $state(false);
  let error = $state<string | null>(null);

  async function release() {
    working = true;
    try {
      const acted = await call(commands.humanRelease(taken ?? [], nextJob()));
      if (acted.notice.error) error = acted.notice.text;
      else onleave();
    } catch (failure) {
      error = message(failure);
    } finally {
      working = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (working) return;
    if (event.key === "k") onleave();
    else if (event.key === "u") void release();
    else return;
    event.preventDefault();
  }
</script>

<AlertDialog.Root open={taken !== null} onOpenChange={(open) => !open && onstay()}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="overlay" />
    <AlertDialog.Content class="dialog" onkeydown={keydown}>
      <AlertDialog.Title class="dialog-title">Unsubmitted claims</AlertDialog.Title>
      <AlertDialog.Description>
        This review claimed {taken?.length === 1 ? "a request" : `${taken?.length} requests`} without submitting:
      </AlertDialog.Description>
      <ul>
        {#each taken ?? [] as id (id)}<li><code>{id}</code></li>{/each}
      </ul>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="dialog-actions">
        <AlertDialog.Cancel class="button">Stay <kbd>Esc</kbd></AlertDialog.Cancel>
        <button class="button" disabled={working} onclick={() => void release()}>Release and quit <kbd>u</kbd></button>
        <AlertDialog.Action class="button primary" disabled={working} onclick={onleave}>Keep and quit <kbd>k</kbd></AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .error {
    color: var(--red);
  }
</style>
