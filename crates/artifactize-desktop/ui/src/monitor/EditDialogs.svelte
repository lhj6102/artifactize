<script lang="ts">
  // The edit dialogs of a Current: New Artifact (a name and its Path, from the tool strip),
  // Rename and Delete (from an Artifact's Definition). Enter creates or renames and Esc
  // cancels; Delete is done only by its button. A refusal is said under the field it concerns,
  // or under Delete's sentence, and the dialog stays with what was typed.
  import { Dialog } from "bits-ui";
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";

  let { branch, ondeleted }: { branch: BranchKey | null; ondeleted: () => void } = $props();
  const { definitions: defs } = getAppState();

  /** What the dialog holds: its name and Path typed, and why the last try wrote nothing. */
  const form = defs.dialogForm;
  const open = $derived(defs.dialog !== null);

  /** The field a refusal concerns: the Path's when it names the path, else the name's. */
  const fieldOf = (text: string): "name" | "path" => (/\bpath\b/i.test(text) && !/\bname\b/i.test(text) ? "path" : "name");

  /** A dialog opens empty, Rename with the name it renames. */
  function reset() {
    const dialog = defs.dialog;
    form.name = dialog?.kind === "rename" ? dialog.name : "";
    form.path = "";
    form.refused = null;
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
      const [name, path] = [form.name.trim(), form.path.trim()];
      const why = await defs.edit(branch, { kind: "create", name, path }, "dialog");
      if (why !== null) form.refused = { field: fieldOf(why), text: why };
      else {
        defs.created = { name, centre: true };
        defs.dialog = null;
      }
    } else if (dialog.kind === "rename") {
      const name = form.name.trim();
      const why = await defs.edit(branch, { kind: "rename", old: dialog.name, new: name }, "dialog");
      if (why !== null) form.refused = { field: "name", text: why };
      else {
        // Its Detail goes on with the new name once the graph draws it.
        defs.created = { name, centre: false };
        defs.dialog = null;
      }
    }
  }

  async function remove() {
    const dialog = defs.dialog;
    if (dialog?.kind !== "delete" || !branch || defs.busy) return;
    const why = await defs.edit(branch, { kind: "delete", name: dialog.name }, "dialog");
    if (why !== null) form.refused = { field: "delete", text: why };
    else {
      defs.dialog = null;
      ondeleted();
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
        {#if form.refused}<p class="refusal" role="alert">{form.refused.text}</p>{/if}
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
              <input class="field-input text" bind:value={form.name} autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={form.refused?.field === "name"} />
            </label>
            {#if form.refused?.field === "name"}<p class="refusal" role="alert">{form.refused.text}</p>{/if}
            <label class="field">
              <span class="label">Path</span>
              <input class="field-input" bind:value={form.path} autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={form.refused?.field === "path"} />
            </label>
            {#if form.refused?.field === "path"}<p class="refusal" role="alert">{form.refused.text}</p>{/if}
          {:else}
            <input class="field-input text" bind:value={form.name} aria-label="Name" autocomplete="off" spellcheck="false" onkeydown={enter} aria-invalid={form.refused !== null} />
            {#if form.refused}<p class="refusal" role="alert">{form.refused.text}</p>{/if}
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
    overflow-wrap: anywhere;
  }
  /* A long name wraps inside the dialog. */
  :global(.dialog.edit .dialog-title) {
    overflow-wrap: anywhere;
  }
  .refusal {
    margin: 4px 0 0;
    color: var(--red);
    font-size: var(--text-small);
  }
</style>
