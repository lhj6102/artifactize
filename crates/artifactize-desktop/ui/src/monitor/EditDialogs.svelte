<script lang="ts">
  // The edit dialogs of a Current: New Artifact (a name and its Path, from the tool strip),
  // Rename and Delete (from an Artifact's Definition). Enter creates or renames and Esc
  // cancels; Delete is done only by its button. A refusal is said under the field it concerns,
  // or under Delete's sentence, and the dialog stays with what was typed.
  import { Dialog } from "bits-ui";
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";

  let { branch, ondeleted }: { branch: BranchKey | null; ondeleted: (name: string) => void } = $props();
  const { definitions: defs } = getAppState();

  let name = $state("");
  let path = $state("");
  /** Why the last try wrote nothing, and which field it concerns. */
  let refused = $state<{ field: "name" | "path" | "delete"; text: string } | null>(null);
  const open = $derived(defs.dialog !== null);

  /** The field a refusal concerns: the Path's when it names the path, else the name's. */
  const fieldOf = (text: string): "name" | "path" => (/\bpath\b/i.test(text) && !/\bname\b/i.test(text) ? "path" : "name");

  /** A dialog opens empty, Rename with the name it renames. */
  function reset() {
    const dialog = defs.dialog;
    name = dialog?.kind === "rename" ? dialog.name : "";
    path = "";
    refused = null;
  }

  /** Enter in a field does what the primary button does, outside a composition. */
  function enter(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || composing(event)) return;
    event.preventDefault();
    void submit();
  }

  async function submit(event?: SubmitEvent) {
    event?.preventDefault();
    const dialog = defs.dialog;
    if (!dialog || !branch || defs.busy) return;
    if (dialog.kind === "new") {
      const why = await defs.edit(branch, { kind: "create", name: name.trim(), path: path.trim() }, "dialog");
      if (why !== null) refused = { field: fieldOf(why), text: why };
      else {
        defs.created = { name: name.trim(), centre: true };
        defs.dialog = null;
      }
    } else if (dialog.kind === "rename") {
      const why = await defs.edit(branch, { kind: "rename", old: dialog.name, new: name.trim() }, "dialog");
      if (why !== null) refused = { field: "name", text: why };
      else {
        // Its Detail goes on with the new name once the graph draws it.
        defs.created = { name: name.trim(), centre: false };
        defs.dialog = null;
      }
    }
  }

  async function remove() {
    const dialog = defs.dialog;
    if (dialog?.kind !== "delete" || !branch || defs.busy) return;
    const why = await defs.edit(branch, { kind: "delete", name: dialog.name }, "dialog");
    if (why !== null) refused = { field: "delete", text: why };
    else {
      defs.dialog = null;
      ondeleted(dialog.name);
    }
  }
</script>

<Dialog.Root
  open={open}
  onOpenChange={(next) => {
    if (!next) defs.dialog = null;
  }}
>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog edit" onOpenAutoFocus={reset}>
      {#if defs.dialog?.kind === "delete"}
        <Dialog.Title class="dialog-title">Delete {defs.dialog.name}?</Dialog.Title>
        <p class="sentence">{defs.node(defs.dialog.name)?.definition ?? defs.dialog.name} is removed.</p>
        {#if refused}<p class="refusal" role="alert">{refused.text}</p>{/if}
        <div class="dialog-actions">
          <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
          <button class="button primary" type="button" disabled={defs.busy} onclick={() => void remove()}>Delete</button>
        </div>
      {:else if defs.dialog}
        <Dialog.Title class="dialog-title">{defs.dialog.kind === "new" ? "New Artifact" : `Rename ${defs.dialog.name}`}</Dialog.Title>
        <form onsubmit={(event) => void submit(event)}>
          {#if defs.dialog.kind === "new"}
            <label class="field">
              <span class="label">Name</span>
              <input class="field-input text" bind:value={name} autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={refused?.field === "name"} />
            </label>
            {#if refused?.field === "name"}<p class="refusal" role="alert">{refused.text}</p>{/if}
            <label class="field">
              <span class="label">Path</span>
              <input class="field-input" bind:value={path} autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={refused?.field === "path"} />
            </label>
            {#if refused?.field === "path"}<p class="refusal" role="alert">{refused.text}</p>{/if}
          {:else}
            <input class="field-input text" bind:value={name} aria-label="Name" autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={refused !== null} />
            {#if refused}<p class="refusal" role="alert">{refused.text}</p>{/if}
          {/if}
          <div class="dialog-actions">
            <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
            <button class="button primary" type="submit" aria-keyshortcuts="Enter" disabled={defs.busy}>{defs.dialog.kind === "new" ? "Create" : "Rename"}</button>
          </div>
        </form>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.dialog.edit) {
    width: min(400px, 92vw);
  }
  .field {
    display: grid;
    gap: 4px;
    margin-top: 8px;
  }
  .field:first-child {
    margin-top: 0;
  }
  .label {
    font-weight: 600;
  }
  .sentence {
    margin: 0 0 8px;
  }
  .refusal {
    margin: 4px 0 0;
    color: var(--red);
    font-size: var(--text-small);
  }
</style>
