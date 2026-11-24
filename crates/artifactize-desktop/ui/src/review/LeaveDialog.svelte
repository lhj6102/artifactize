<script lang="ts">
  // Quitting with claims this window took and did not submit, or with a Chat turn running
  // (J-QUIT): stay, or release the claims and quit, which also stops the turn. Its two keys
  // show as hints on its buttons, the one dialog that shows its keys: Esc stays, `u` releases
  // the claims and quits.
  import { LeaveDialogState, setLeaveDialogState } from "./leave-dialog-state.svelte";
  import { AlertDialog } from "bits-ui";
  import { useIpc, call, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";
  import type { Held } from "../shell/leaving.svelte";

  let { held, onstay, onleave }: { held: Held | null; onstay: () => void; onleave: () => void } = $props();
  const commands = useIpc();
  const l = new LeaveDialogState();
  setLeaveDialogState(l);
  const app = getAppState();
  const taken = $derived(held?.taken ?? []);
  const turns = $derived(held?.turns ?? []);

  async function release() {
    // Nothing held: quitting stops the running turn, as Stop would.
    if (taken.length === 0) return onleave();
    l.working = true;
    try {
      // Only the claims this app took are released, all of them or none.
      const acted = await call(commands.humanReleaseTaken(taken, app.jobs.next()));
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
    if (event.key === "u") void release();
    else return;
    event.preventDefault();
  }
</script>

<AlertDialog.Root open={held !== null} onOpenChange={(open) => !open && onstay()}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="overlay" />
    <AlertDialog.Content class="dialog" onkeydown={keydown}>
      <AlertDialog.Title class="dialog-title">{taken.length > 0 ? "Leave the review?" : "Quit while the agent works?"}</AlertDialog.Title>
      <div class="dialog-body">
        <AlertDialog.Description class="held">
          {#if taken.length > 0}
            This window holds {taken.length === 1 ? "1 review claim" : `${taken.length} review claims`}: {taken.join(", ")}.
          {/if}
          {#if turns.length > 0}
            A Chat turn is running in {turns.join(", ")}; quitting stops it.
          {/if}
        </AlertDialog.Description>
        {#if l.error}<p class="error" role="alert">{l.error}</p>{/if}
      </div>
      <div class="dialog-actions">
        <AlertDialog.Cancel class="button" aria-keyshortcuts="Escape">Stay <kbd>Esc</kbd></AlertDialog.Cancel>
        <button class="button primary" aria-keyshortcuts="u" disabled={l.working} onclick={() => void release()}>{taken.length > 0 ? "Release and quit" : "Quit"} <kbd>u</kbd></button>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .error {
    color: var(--red);
  }
  :global(.held) {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
