<script lang="ts">
  // The one left column: the Search at the top, the live Branches below it and the Waiting
  // stack at the bottom. A branch reads as `project / branch` with its newest Run's status;
  // expanded, it lists its timeline under its name. ↑ ↓ move one selection through the
  // branches, the expanded timelines and then the Waiting stack; → expands the selected branch
  // or enters its timeline, ← steps back out or folds it; Enter goes to the Main area, or opens
  // the selected waiting review. The stack is a thin bar with no review waiting and grows
  // upward with each one, up to half the Pane, beyond which it scrolls.
  import type { BranchView, Chip, WaitingRow } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import SearchField from "../shell/SearchField.svelte";
  import { scrollTo } from "../shell/attach";
  import { plain } from "../shell/keys";
  import { tone, toneColour } from "../shell/tone";
  import Timeline from "./Timeline.svelte";
  import { entryKey, keyOf } from "./monitor-state.svelte";

  let {
    query = $bindable(),
    filled,
    onselect,
    ontoggle,
    onchoose,
    onopen,
    onmove,
    onstep,
    onmain,
  }: {
    /** The search as typed. */
    query: string;
    /** The timeline entry of each branch shown in the Main area, by its index. */
    filled: (branch: BranchView) => number | null;
    /** A click on a branch: it expands and its newest entry shows. */
    onselect: (branch: BranchView) => void;
    /** Expand a branch, or fold it back. */
    ontoggle: (branch: BranchView) => void;
    /** Show an entry of a branch's timeline. */
    onchoose: (branch: BranchView, chip: Chip) => void;
    /** Open a waiting review in the Main area. */
    onopen: (row: WaitingRow) => void;
    /** Move the selection by one entry. */
    onmove: (by: number) => void;
    /** → (1) or ← (-1) on the selection. */
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

  /** The selected entry's element, kept in view. */
  const selectedKey = $derived(entryKey(selected));
  const isSelected = (key: string) => selectedKey === key;
  /** The selected entry of `branch`'s timeline, by its name. */
  const selectedChip = (branch: BranchView) =>
    selected?.kind === "chip" && keyOf(selected.branch) === keyOf(branch.key) ? selected.chip : null;
  /** A collapsed branch's line under its name: its newest Run, the one in progress first. */
  const latest = (branch: BranchView) => branch.chips.find((chip) => chip.kind === "run") ?? null;

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
    return branch.name ? `${branch.project} / ${branch.name}` : branch.project;
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
    <ul class="branches" role="tree" aria-label="Branches">
      {#each branches as branch (keyOf(branch.key))}
        {@const key = keyOf(branch.key)}
        {@const open = m.expanded.has(key)}
        {@const newest = latest(branch)}
        <li role="treeitem" aria-expanded={open} aria-selected={isSelected(key)} class="branch" class:open data-key={key}>
          <div class="head" class:selected={isSelected(key)}>
            <button class="chevron" tabindex="-1" aria-label="{open ? 'Fold' : 'Expand'} {name(branch)}" onclick={() => ontoggle(branch)}
              ><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3.5 10.5 8 6 12.5" /></svg></button
            >
            <button class="name" tabindex="-1" onclick={() => onselect(branch)}>
              <span class="label"
                >{#if branch.name}<span class="project">{branch.project} /</span> <span class="branch-name">{branch.name}</span
                  >{:else}<span class="branch-name">{branch.project}</span>{/if}</span
              >
              {#if !open}
                <span class="latest">
                  {#if newest?.kind === "run"}
                    <span class="dot" style:--dot={toneColour(newest.mark.status)} aria-hidden="true"></span>
                    <span class="status {tone(newest.mark.status)}">{newest.status}</span>
                    <span class="age">{newest.age}</span>
                  {:else}
                    <span class="none">No Runs yet</span>
                  {/if}
                </span>
              {/if}
            </button>
            <button
              class="star"
              class:on={branch.starred}
              tabindex="-1"
              aria-label="Star {name(branch)}"
              aria-pressed={branch.starred}
              onclick={() => void browse.star(branch.key, !branch.starred)}>{branch.starred ? "★" : "☆"}</button
            >
          </div>
          {#if open}
            <Timeline
              {branch}
              filled={filled(branch)}
              selected={selectedChip(branch)}
              onchoose={(chip) => onchoose(branch, chip)}
              onolder={() => void browse.older(branch.key)}
            />
          {/if}
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
  .branch {
    min-width: 0;
    padding: 6px var(--pad) 8px 6px;
    border-bottom: 1px solid var(--border);
  }
  /* A chevron, then the name over its newest Run's status, then the star. */
  .head {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    align-items: start;
    column-gap: 4px;
    min-width: 0;
    border-radius: 8px;
  }
  .chevron {
    width: 22px;
    height: 22px;
    margin: 0;
    padding: 3px;
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .chevron svg {
    display: block;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 120ms ease;
  }
  .open .chevron svg {
    transform: rotate(90deg);
  }
  .name {
    display: grid;
    gap: 3px;
    min-width: 0;
    margin: 0;
    padding: 2px 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  /* The branch reads first; its project is the quieter half of the name. */
  .label {
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .project {
    color: var(--muted);
  }
  .branch-name {
    font-weight: 600;
  }
  .latest {
    display: grid;
    grid-template-columns: 10px auto minmax(0, 1fr);
    align-items: center;
    column-gap: 8px;
    font-size: 12px;
  }
  .latest .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--dot);
  }
  .latest .status {
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .latest .age {
    justify-self: end;
    color: var(--muted);
    white-space: nowrap;
  }
  .latest .none {
    grid-column: 1 / -1;
    color: var(--muted);
  }
  /* A star is quiet until it is on; on, it carries the amber of attention. */
  .star {
    margin: 2px 0 0;
    padding: 0 2px;
    border: 0;
    background: none;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.4;
    cursor: pointer;
    opacity: 0.55;
  }
  .star:hover,
  .star.on {
    opacity: 1;
  }
  .star.on {
    color: var(--light-active);
  }
  .branch :global(.timeline) {
    margin-left: 26px;
  }
  .branch :global(.older) {
    margin-left: 54px;
  }
  /* A selected timeline entry is the one shown, already in its grey pill; keys add the bar. */
  .selected {
    background: var(--selection);
  }
  .entries:focus-visible .selected,
  .entries:focus-visible .branch :global(li.selected > .entry) {
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
