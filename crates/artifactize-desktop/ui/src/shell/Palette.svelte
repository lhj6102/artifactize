<script lang="ts">
  // Ctrl+K (⌘K): every action that is possible now, searchable, with its shortcut.
  import { Command, Dialog } from "bits-ui";
  import { registry } from "./commands.svelte";
  import { label } from "./keys";

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let input = $state<HTMLInputElement | null>(null);

  const groups = $derived.by(() => {
    const enabled = registry.all.filter((command) => command.enabled?.() ?? true);
    const names = [...new Set(enabled.map((command) => command.group))];
    return names.map((name) => ({
      name,
      commands: enabled.filter((command) => command.group === name),
    }));
  });

  function run(id: string) {
    const command = registry.all.find((command) => command.id === id);
    open = false;
    // Let the dialog restore focus before the command moves it.
    queueMicrotask(() => command?.run());
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content
      class="dialog palette"
      aria-label="Command palette"
      onOpenAutoFocus={(event) => {
        // However it opened (button or Ctrl+K), typing searches the commands.
        event.preventDefault();
        input?.focus();
      }}
    >
      <Dialog.Title class="sr-only">Command palette</Dialog.Title>
      <Command.Root loop>
        <Command.Input class="palette-input" placeholder="Type a command…" bind:ref={input} />
        <Command.List class="palette-list">
          <Command.Viewport>
            <Command.Empty class="palette-empty">No matching command.</Command.Empty>
            {#each groups as group (group.name)}
              <Command.Group value={group.name}>
                <Command.GroupHeading class="palette-heading">{group.name}</Command.GroupHeading>
                <Command.GroupItems>
                  {#each group.commands as command (command.id)}
                    <Command.Item
                      class="palette-item"
                      value={command.id}
                      keywords={[command.label]}
                      onSelect={() => run(command.id)}
                    >
                      <span>{command.label}</span>
                      {#if command.keys}<kbd>{label(command.keys)}</kbd>{/if}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            {/each}
          </Command.Viewport>
        </Command.List>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
