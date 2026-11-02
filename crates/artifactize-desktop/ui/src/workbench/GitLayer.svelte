<script lang="ts">
  // The window's far-left column: the shown worktree's history as a vertical line with a dot
  // for each commit, newest at the top, the checked-out one ringed and merges hollow. A long
  // history shows its newest commits first and more on request. Each commit offers a new
  // worktree from it: a sibling folder of the primary checkout on a generated branch, which
  // opens once the Pane lists it.
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import type { Commit, WorktreeKey } from "./backend";
  import { PAGE } from "./git-layer.svelte";

  let {
    worktree,
    outside = false,
  }: {
    /** The worktree whose history shows, or `null` with none shown. */
    worktree: WorktreeKey | null;
    /** The shown folder is outside Git: it has no history to show. */
    outside?: boolean;
  } = $props();
  const { git } = getAppState();

  const commits = $derived(worktree !== null && git.shown?.worktree === worktree ? git.shown.commits : null);
  const created = $derived(git.created);

  /** Read the history for every other worktree and whenever asked again. */
  const reread: Attachment<HTMLElement> = () => {
    void git.asked;
    if (worktree !== null) void git.read(worktree);
  };

  const short = (commit: Commit) => commit.id.slice(0, 7);
  const when = (commit: Commit) => new Date(commit.time).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
</script>

<section class="git" aria-label="Git layer" {@attach reread}>
  <header>
    <h2>History</h2>
    {#if worktree !== null}
      <button class="again" aria-label="Read the history again" title="Read the history again" onclick={() => git.asked++}>↻</button>
    {/if}
  </header>
  {#if git.error && worktree !== null}<p class="failure" role="status">{git.error}</p>{/if}
  {#if created}
    <p class="made" role="status">
      Created <code>{created.branch}</code> in <code>{created.worktree}</code>{git.opening === null ? "." : "; it opens once listed."}
    </p>
  {/if}
  {#if outside}
    <p class="empty">This folder is outside Git: it has no history and no worktrees.</p>
  {:else if worktree === null}
    <p class="empty">Choose a worktree to see its history.</p>
  {:else if commits === null}
    {#if !git.error}<p class="empty">Reading the history…</p>{/if}
  {:else if commits.length === 0}
    <p class="empty">No commits yet.</p>
  {:else}
    <ol class="commits" aria-label="Commits">
      {#each commits.slice(0, git.limit) as commit (commit.id)}
        <li
          class="commit"
          class:head={commit.head}
          class:merge={commit.parents.length > 1}
          class:root={commit.parents.length === 0}
          title="{short(commit)} {commit.summary}&#10;{when(commit)}"
        >
          <span class="dot" aria-hidden="true"></span>
          <span class="text">
            {#if commit.refs.length > 0}
              <span class="refs">{#each commit.refs as ref (`${ref.kind} ${ref.name}`)}<span class="ref {ref.kind}" title={ref.kind}>{ref.name}</span>{/each}</span>
            {/if}
            <span class="subject">{commit.summary}</span>
            <span class="meta"><code>{short(commit)}</code></span>
          </span>
          <button
            class="make"
            aria-label="New worktree from {short(commit)}"
            title="New worktree from {short(commit)}"
            disabled={git.creating !== null}
            onclick={() => worktree !== null && void git.create(worktree, commit.id)}>+</button
          >
        </li>
      {/each}
      {#if commits.length > git.limit}
        <li class="more">
          <button class="button" onclick={() => (git.limit += PAGE)}>Show {Math.min(PAGE, commits.length - git.limit)} older commits</button>
        </li>
      {/if}
    </ol>
  {/if}
</section>

<style>
  .git {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    padding-top: 10px;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px 6px 12px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .again {
    padding: 0 4px;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .again:hover {
    color: var(--text);
  }
  .empty,
  .failure,
  .made {
    margin: 0;
    padding: 4px 12px;
    font-size: 12px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .failure {
    color: var(--red);
  }
  .made code {
    font: 11px var(--mono);
  }
  /* The line runs through every dot, from the newest commit to the oldest shown. */
  .commits {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 0 0 8px;
  }
  .commit {
    position: relative;
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: start;
    padding: 4px 6px 4px 0;
    min-width: 0;
  }
  .commit::before {
    content: "";
    position: absolute;
    left: 13px;
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--border);
  }
  .commit:first-child::before {
    top: 11px;
  }
  .commit.root::before,
  .commit:last-child::before {
    bottom: calc(100% - 11px);
  }
  .more {
    padding: 6px 12px;
  }
  .more .button {
    font-size: 12px;
    padding: 2px 10px;
  }
  .commit:hover {
    background: var(--surface-2);
  }
  .dot {
    position: relative;
    justify-self: center;
    width: 10px;
    height: 10px;
    margin-top: 3px;
    border-radius: 50%;
    background: var(--accent);
    border: 2px solid var(--accent);
  }
  .merge .dot {
    background: var(--surface);
  }
  .head .dot {
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .text {
    display: grid;
    gap: 1px;
    min-width: 0;
    font-size: 12px;
  }
  .refs {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }
  .ref {
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 11px;
    white-space: nowrap;
  }
  /* A tag is outlined; a remote's branch and any other ref are quieter. */
  .ref.tag {
    background: none;
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .ref.remote,
  .ref.other {
    background: var(--surface-2);
    color: var(--muted);
  }
  .subject {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--muted);
  }
  .meta code {
    font: 11px var(--mono);
  }
  /* A commit's action is quiet until its row is hovered or it has focus; the keyboard reaches
   * it with Tab. */
  .make {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--muted);
    font: inherit;
    line-height: 1;
    cursor: pointer;
    opacity: 0.35;
  }
  .commit:hover .make,
  .make:focus-visible {
    opacity: 1;
  }
  .make:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .make:disabled {
    cursor: default;
  }
</style>
