<script lang="ts">
  // Chat: its Pane lists the sessions and its main area shows the chosen one. The sessions come
  // from the library's agent harness; until this build has it, the place says so.
  import Panel from "../shell/Panel.svelte";

  let focused = $state(false);
</script>

<div class="mode">
  <Panel title="Chat" label="Chat's main area" {focused} onfocus={() => (focused = true)}>
    <p class="empty">Chat sessions are not available in this build.</p>
  </Panel>
</div>

<style>
  .mode {
    height: 100%;
    min-height: 0;
  }
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
</style>
