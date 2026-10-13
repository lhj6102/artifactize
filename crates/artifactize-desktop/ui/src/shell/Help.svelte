<script lang="ts">
  // The keys of the focused screen, like the terminal monitor's `?` overlay.
  import { Dialog } from "bits-ui";
  import { label } from "./keys";

  let {
    open = $bindable(false),
    sections,
  }: { open?: boolean; sections: { title: string; keys: [string, string][] }[] } = $props();
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog help">
      <Dialog.Title class="dialog-title">Keyboard shortcuts</Dialog.Title>
      <div class="help-grid">
        {#each sections as section (section.title)}
          <section>
            <h3>{section.title}</h3>
            <dl>
              {#each section.keys as [keys, action] (keys)}
                <dt><kbd>{label(keys)}</kbd></dt>
                <dd>{action}</dd>
              {/each}
            </dl>
          </section>
        {/each}
      </div>
      <div class="dialog-actions">
        <Dialog.Close class="button">Close</Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
