<script lang="ts">
  // Every repository and worktree in the state; selecting one reads its Runs.
  import type { Browse } from "./browse.svelte";
  import type { ScopeRow } from "../ipc/client";
  import Panel from "../shell/Panel.svelte";
  import { scrollTo } from "../shell/attach";
  import Badge from "../shell/Badge.svelte";
  import { plain } from "../shell/keys";
  import { commands } from "../ipc/client";

  let {
    browse,
    focused,
    element = $bindable(),
    onfocus,
    onnext,
  }: {
    browse: Browse;
    focused: boolean;
    element?: HTMLElement;
    onfocus: () => void;
    onnext: () => void;
  } = $props();

  const rows = $derived(browse.view?.scopes ?? []);
  /** The highlighted row: the selected scope, or a fold row the keys moved to. */
  let cursor = $state<number | null>(null);
  const index = $derived(cursor ?? Math.max(rows.findIndex((row) => row.selected), 0));

  async function choose(at: number) {
    const row = rows[at];
    if (!row) return;
    cursor = row.fold ? at : null;
    if (!row.fold && !row.selected) await browse.navigate({ scope: row.scope });
  }

  async function toggle(row: ScopeRow) {
    if (!row.fold) return;
    browse.view = await commands.toggleWorktrees(row.fold);
  }

  function keydown(event: KeyboardEvent) {
    if (plain(event, "ArrowDown") || plain(event, "j")) void choose(Math.min(index + 1, rows.length - 1));
    else if (plain(event, "ArrowUp") || plain(event, "k")) void choose(Math.max(index - 1, 0));
    else if (plain(event, " ") && rows[index]?.fold) void toggle(rows[index]);
    else if (plain(event, "Enter") || plain(event, "ArrowRight")) onnext();
    else if (plain(event, "r")) browse.refresh();
    else return;
    event.preventDefault();
  }
</script>

<Panel title="Scope" {focused} bind:element {onfocus} onkeydown={keydown}>
  <ul class="rows" role="listbox" aria-label="Scope" tabindex="0" {@attach scrollTo(() => `[data-index="${index}"]`)}>
    {#each rows as row, at (at)}
      <li
        role="option"
        aria-selected={at === index}
        class:selected={at === index}
        class:fold={row.fold !== null}
        data-index={at}
        style:padding-left="{16 + row.depth * 16}px"
        onclick={() => (row.fold ? (void choose(at), void toggle(row)) : void choose(at))}
        onkeydown={() => {}}
      >
        <span class="label">{row.label}</span>
        <Badge counts={row.badge} />
      </li>
    {/each}
  </ul>
</Panel>

<style>
  .rows {
    list-style: none;
    margin: 0;
    padding: 0 0 8px;
    outline: none;
  }
  li {
    display: flex;
    gap: 8px;
    justify-content: space-between;
    padding: 5px 16px;
    cursor: pointer;
    white-space: nowrap;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  li:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  .rows:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .fold {
    color: var(--muted);
  }
</style>
