<script lang="ts">
  // Help: the keys of what has focus, each a key cap beside what it does, and for a graph what
  // each of its marks means, each status's icon in its colour beside its word, the one place
  // that spells them out.
  import { Dialog } from "bits-ui";
  import type { Light } from "../ipc/client";
  import { WORDS } from "../monitor/graph";
  import Icon from "./Icon.svelte";
  import { LIGHTS } from "./icons";
  import { label } from "./keys";
  import type { Section } from "./shortcuts";

  let {
    open = $bindable(false),
    sections,
    marks,
  }: { open?: boolean; sections: Section[]; marks: Light[] } = $props();
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
        {#if marks.length > 0}
          <section>
            <h3>Marks</h3>
            <ul class="marks" aria-label="Marks">
              {#each marks as light (light)}
                <li class="light-{light}"><span class="mark"><Icon name={LIGHTS[light].icon} /></span>{WORDS[light]}</li>
              {/each}
            </ul>
          </section>
        {/if}
      </div>
      <div class="dialog-actions">
        <Dialog.Close class="button" aria-keyshortcuts="Escape">Close</Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .marks {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px 16px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .marks li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .mark {
    display: inline-flex;
    color: var(--light);
  }
</style>
