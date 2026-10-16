<script lang="ts">
  // Quitting with claims this window took and did not submit: keep or release them.
  import { LeaveDialogState, setLeaveDialogState } from "./leave-dialog-state.svelte";
  import { AlertDialog } from "bits-ui";
  import { useIpc, call, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";

  let {
    taken,
    onstay,
    onleave,
  }: { taken: string[] | null; onstay: () => void; onleave: () => void } = $props();
  const commands = useIpc();
  const l = new LeaveDialogState();
  setLeaveDialogState(l);
  const app = getAppState();

  async function release() {
    l.working = true;
    try {
      const acted = await call(commands.humanRelease(taken ?? [], app.jobs.next()));
      if (acted.notice.error) l.error = acted.notice.text;
      else onleave();
    } catch (failure) {
      l.error = message(failure);
    } finally {
      l.working = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (l.working || composing(event)) return;
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
      {#if l.error}<p class="error" role="alert">{l.error}</p>{/if}
      <div class="dialog-actions">
        <AlertDialog.Cancel class="button">Stay <kbd>Esc</kbd></AlertDialog.Cancel>
        <button class="button" disabled={l.working} onclick={() => void release()}>Release and quit <kbd>u</kbd></button>
        <AlertDialog.Action class="button primary" disabled={l.working} onclick={onleave}>Keep and quit <kbd>k</kbd></AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .error {
    color: var(--red);
  }
</style>
