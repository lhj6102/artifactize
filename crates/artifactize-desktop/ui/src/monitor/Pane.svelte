<script lang="ts">
  // Verify's left column: the Search at the top and the live Branches below it. A branch reads
  // as `project / branch` with its newest Run's status; expanded, it lists its timeline under
  // its name. ↑ ↓ move one selection through the branches and the expanded timelines; →
  // expands the selected branch or enters its timeline, ← steps back out or folds it; Enter goes
  // to the Main area. The reviews waiting for a person are Review's.
  import type { BranchView, Chip } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
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
  const selected = $derived(m.selected);
  const invalid = $derived(view?.query.error ?? null);

  /** The selected entry's element, kept in view. */
  const selectedKey = $derived(entryKey(selected));
  const isSelected = (key: string) => selectedKey === key;
  /** The selected entry of `branch`'s timeline, by its name. */
  const selectedChip = (branch: BranchView) =>
    selected?.kind === "chip" && keyOf(selected.branch) === keyOf(branch.key) ? selected.chip : null;
  /** A collapsed branch's line under its name: its newest Run, the one in progress first. */
  const latest = (branch: BranchView) => branch.newest;

  function keydown(event: KeyboardEvent) {
    if (plain(event, "ArrowDown") || plain(event, "j")) onmove(1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) onmove(-1);
    else if (plain(event, "ArrowLeft")) onstep(-1);
    else if (plain(event, "ArrowRight")) onstep(1);
    else if (plain(event, "Enter") && selected) onmain();
    else return;
    event.preventDefault();
  }

  function name(branch: BranchView): string {
    const name = branch.name ? `${branch.project} / ${branch.name}` : branch.project;
    return branch.historical ? `${name} (saved Run, no checkout)` : name;
  }
</script>

<section class="pane" class:focused={m.focus === "pane"} aria-label="Pane" onfocusin={() => (m.focus = "pane")}>
  <SearchField
    bind:value={query}
    bind:input={m.panes.search}
    label="Search the branches"
    placeholder="Search: text, repo:, branch:, sort:recent, sort:name"
    reset={browse.start}
    complete={(text) => browse.complete(text)}
    onleave={() => m.panes.pane?.focus()}
  />
  <!-- The keys move one selection through the branches; they bubble up from any entry. -->
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
          <div class="head" class:selected={isSelected(key)} class:has-latest={!open && newest?.kind === "run"}>
            <button class="chevron" tabindex="-1" aria-label="{open ? 'Fold' : 'Expand'} {name(branch)}" onclick={() => ontoggle(branch)}
              ><Icon name={open ? "unfold" : "fold"} /></button
            >
            <button class="name" tabindex="-1" onclick={() => onselect(branch)}>
              <span class="label"
                >{#if branch.name}<span class="project">{branch.project} /</span> <span class="branch-name">{branch.name}</span
                  >{:else}<span class="branch-name">{branch.project}</span>{/if}{#if branch.historical} <span class="project">(saved)</span>{/if}</span
              >
              <!-- Folded, the newest Run's status under the name; a branch without Runs says
                   nothing (D-SYS-11). -->
              {#if !open && newest?.kind === "run"}
                <span class="latest">
                  <span class="dot" style:--dot={toneColour(newest.mark.status)} aria-hidden="true"></span>
                  <span class="status {tone(newest.mark.status)}">{newest.status}</span>
                  <span class="age">{newest.age}</span>
                </span>
              {/if}
            </button>
            <button
              class="star"
              class:on={branch.starred}
              tabindex="-1"
              aria-label="Star {name(branch)}"
              aria-pressed={branch.starred}
              onclick={() => void browse.star(branch.key, !branch.starred)}><Icon name={branch.starred ? "star-on" : "star"} /></button
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
  </div>
</section>

<style>
  .pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--surface-panel);
  }
  .entries {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    outline: none;
  }
  /* Keyboard focus follows the row; it is not part of its selection paint. */
  .entries:focus-visible:not(:has(.head.selected, :global(li.selected))),
  .entries:focus-visible .head.selected,
  .entries:focus-visible :global(li.selected > .entry) {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
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
    border-bottom: 1px solid var(--line-group);
  }
  /* A chevron, then the name over its newest Run's status, then the star. */
  .head {
    position: relative;
    height: var(--row-pane);
  }
  /* Hover only highlights the branch under the pointer, and the control under it. */
  .head:not(.selected):hover {
    background: var(--surface-group);
  }
  /* The fold icon in a 20px box, the unfold icon while expanded. */
  .chevron {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    position: absolute;
    top: 50%;
    left: 6px;
    transform: translateY(-50%);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .name {
    display: grid;
    align-content: center;
    grid-template-columns: minmax(0, 1fr);
    width: 100%;
    height: 100%;
    min-width: 0;
    margin: 0;
    padding: 0 calc(var(--pad) + var(--control)) 0 30px;
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    line-height: 15px;
    text-align: left;
    cursor: pointer;
  }
  .head.selected {
    background: var(--selection);
  }
  .head.selected .name {
    color: var(--ink);
  }
  /* The branch reads first; its project is the quieter half of the name. */
  .label {
    font-size: var(--text-title);
    line-height: 18px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .project {
    color: var(--ink-2);
  }
  .branch-name {
    font-weight: 700;
  }
  .latest {
    display: grid;
    grid-template-columns: 10px auto minmax(0, 1fr);
    align-items: center;
    column-gap: 8px;
    font-size: var(--text-small);
    line-height: 12px;
  }
  .latest .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--dot);
  }
  .latest .age {
    justify-self: end;
    color: var(--ink-2);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  /* The star is its own 24px target over the branch's full-width button. */
  .star {
    display: grid;
    place-items: center;
    width: var(--control);
    height: var(--control);
    position: absolute;
    top: 50%;
    right: calc(var(--pad) - 4px);
    transform: translateY(-50%);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .has-latest :is(.chevron, .star) {
    top: calc(50% - 6px);
  }
  .star.on {
    color: var(--ink);
  }
  .chevron:hover,
  .star:hover {
    color: var(--ink-bright);
  }
  .empty {
    color: var(--ink-2);
    padding: 0 var(--pad);
  }
  .empty.error {
    color: var(--red);
  }
</style>
