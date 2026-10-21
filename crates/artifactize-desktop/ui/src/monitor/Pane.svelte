<script lang="ts">
  // The one left column: the Search at the top, the live Branches below it and the Waiting
  // stack at the bottom. The keys move one selection through the Branches and then the Waiting
  // stack; ← → step through the selected branch's chips; Enter goes to the Main area, or opens
  // the selected waiting review. The stack is a thin bar with no review waiting and grows
  // upward with each one, up to half the Pane, beyond which it scrolls.
  import type { BranchView, Chip, WaitingRow } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import SearchField from "../shell/SearchField.svelte";
  import { scrollTo } from "../shell/attach";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";
  import ChipRow from "./ChipRow.svelte";
  import { keyOf } from "./monitor-state.svelte";

  let {
    query = $bindable(),
    filled,
    onselect,
    onchoose,
    onopen,
    onmove,
    onstep,
    onmain,
  }: {
    /** The search as typed. */
    query: string;
    /** The chip of each branch shown in the Main area, by branch key. */
    filled: (branch: BranchView) => number | null;
    /** Select a branch: its last chip shows. */
    onselect: (branch: BranchView) => void;
    /** Show a chip of a branch. */
    onchoose: (branch: BranchView, chip: Chip) => void;
    /** Open a waiting review in the Main area. */
    onopen: (row: WaitingRow) => void;
    /** Move the selection by one entry. */
    onmove: (by: number) => void;
    /** Step through the selected branch's chips. */
    onstep: (by: number) => void;
    /** Go to the Main area. */
    onmain: () => void;
  } = $props();
  const app = getAppState();
  const { browse, monitor: m } = app;

  const view = $derived(browse.view);
  const branches = $derived(view?.branches ?? []);
  const waiting = $derived(view?.waiting ?? []);
  const selected = $derived(m.selected);
  const invalid = $derived(view?.query.error ?? null);

  const isSelected = (key: string) => selected !== null && (selected.kind === "branch" ? keyOf(selected.branch) : selected.id) === key;
  /** The selected entry's element, kept in view. */
  const selectedKey = $derived(selected === null ? null : selected.kind === "branch" ? keyOf(selected.branch) : selected.id);

  function keydown(event: KeyboardEvent) {
    if (plain(event, "ArrowDown") || plain(event, "j")) onmove(1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) onmove(-1);
    else if (plain(event, "ArrowLeft")) onstep(-1);
    else if (plain(event, "ArrowRight")) onstep(1);
    else if (plain(event, "Enter")) {
      const row = selected?.kind === "waiting" ? waiting.find((item) => item.id === selected.id) : undefined;
      if (row) onopen(row);
      else if (selected) onmain();
    } else return;
    event.preventDefault();
  }

  function name(branch: BranchView): string {
    return branch.name ? `${branch.project}  ${branch.name}` : branch.project;
  }
</script>

<section class="pane" class:focused={m.focus === "pane"} aria-label="Pane" onfocusin={() => (m.focus = "pane")}>
  <SearchField
    bind:value={query}
    bind:input={m.panes.search}
    label="Search branches and reviews"
    placeholder="Search: text, repo:, branch:, sort:recent, sort:name"
    reset="sort:recent"
    complete={(text) => browse.complete(text)}
    onleave={() => m.panes.pane?.focus()}
  />
  <!-- The keys move one selection through both lists; they bubble up from any entry. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="entries"
    role="group"
    aria-label="Pane entries"
    tabindex="0"
    bind:this={m.panes.pane}
    onkeydown={keydown}
    {@attach scrollTo(() => (selectedKey === null ? ":not(*)" : `[data-key="${CSS.escape(selectedKey)}"]`))}
  >
    <ul class="branches" role="listbox" aria-label="Branches">
      {#each branches as branch (keyOf(branch.key))}
        {@const key = keyOf(branch.key)}
        <li role="option" aria-selected={isSelected(key)} class:selected={isSelected(key)} data-key={key}>
          <button class="name" tabindex="-1" onclick={() => onselect(branch)}>{name(branch)}</button>
          <ChipRow {branch} filled={filled(branch)} onchoose={(chip) => onchoose(branch, chip)} onolder={() => void browse.older(branch.key)} />
        </li>
      {:else}
        <li class="empty" class:error={!!invalid}>
          {invalid ?? (!view?.refreshed ? "Loading…" : "No live branch matches. Start one with `artifactize verify`.")}
        </li>
      {/each}
    </ul>
    <section class="waiting" aria-label="Waiting">
      <h2 class="bar">Waiting ({waiting.length})</h2>
      {#if waiting.length > 0}
        <ul role="listbox" aria-label="Waiting reviews">
          {#each waiting as row (row.id)}
            <li
              role="option"
              aria-selected={isSelected(row.id)}
              class:selected={isSelected(row.id)}
              data-key={row.id}
              onclick={() => onopen(row)}
              onkeydown={() => {}}
            >
              <span class="glyph {tone(row.mark.status)}">{row.mark.glyph}</span>
              <span class="eval">{row.eval}</span>
              <span class="where">{row.branch ? `${row.project}  ${row.branch}` : row.project}</span>
              <span class="meta">{row.age} · {row.claim}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</section>

<style>
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding-top: 10px;
    background: var(--surface);
    border-top: 2px solid transparent;
  }
  .pane.focused {
    border-top-color: var(--accent);
  }
  .entries {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    outline: none;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .branches {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }
  .branches li {
    display: grid;
    gap: 2px;
    padding: 6px var(--pad);
    min-width: 0;
  }
  .name {
    justify-self: start;
    max-width: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-weight: 600;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .selected {
    background: var(--selection);
  }
  .entries:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  /* Grows upward like a stack, up to half the Pane; beyond that it scrolls. */
  .waiting {
    flex: 0 0 auto;
    max-height: 50%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--border);
  }
  .bar {
    margin: 0;
    padding: 4px var(--pad);
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .waiting ul {
    min-height: 0;
    overflow: auto;
  }
  .waiting li {
    display: grid;
    grid-template-columns: 1em minmax(0, 1fr);
    gap: 0 8px;
    padding: 4px var(--pad);
    cursor: pointer;
  }
  .waiting li:hover {
    background: var(--surface-2);
  }
  .waiting li.selected {
    background: var(--selection);
  }
  .eval,
  .where,
  .meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .where,
  .meta {
    grid-column: 2;
    color: var(--muted);
    font-size: 12px;
    white-space: pre;
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .empty.error {
    color: var(--red);
  }
</style>
