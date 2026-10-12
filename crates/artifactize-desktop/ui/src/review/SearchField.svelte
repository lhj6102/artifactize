<script lang="ts">
  // The review list's request search: the query as text, with completion of its last term in a
  // dropdown. ↑/↓ choose a suggestion, Tab or Enter take it, Esc closes the dropdown, then
  // clears the query, then leaves for the list. Keys mid-composition belong to the input method.
  import type { Completion } from "../ipc/client";
  import { SearchFieldState, setSearchFieldState } from "./searchField.svelte";
  import { composing } from "../shell/keys";

  let {
    value = $bindable(),
    input = $bindable(),
    complete,
    onleave,
  }: {
    value: string;
    input?: HTMLInputElement;
    /** Candidates for the last term of a query. */
    complete: (query: string) => Promise<Completion>;
    /** Leave the field for the list. */
    onleave: () => void;
  } = $props();
  const q = new SearchFieldState();
  setSearchFieldState(q);

  
  
  /** Each request supersedes the ones before it, so a slow answer never reopens the dropdown. */
  let asked = 0;

  const open = $derived(q.completion !== null && q.completion.candidates.length > 0);

  /**
   * Ask for candidates of the last term. An empty term, as after a space, offers every key
   * only when `always` (↓, or a key just taken).
   */
  async function suggest(always = false) {
    const ask = ++asked;
    const answer = await complete(value);
    if (ask !== asked || document.activeElement !== input) return;
    q.completion = always || answer.keep !== value ? answer : null;
    q.active = -1;
  }

  function close() {
    asked++;
    q.completion = null;
    q.active = -1;
  }

  /** Put `candidate` in place of the last term; a key keeps the dropdown open for its values. */
  function accept(candidate: string) {
    if (!q.completion) return;
    const key = candidate.endsWith(":");
    value = q.completion.keep + candidate + (key ? "" : " ");
    close();
    if (key) void suggest(true);
  }

  function keydown(event: KeyboardEvent) {
    if (composing(event)) return;
    const candidates = q.completion?.candidates ?? [];
    if (event.key === "ArrowDown") {
      if (open) q.active = (q.active + 1) % candidates.length;
      else void suggest(true);
    } else if (event.key === "ArrowUp" && open) {
      q.active = q.active <= 0 ? candidates.length - 1 : q.active - 1;
    } else if ((event.key === "Tab" || event.key === "Enter") && open && !event.shiftKey) {
      accept(candidates[Math.max(q.active, 0)]);
    } else if (event.key === "Enter") {
      onleave();
    } else if (event.key === "Escape") {
      if (open) close();
      else if (value !== "") value = "";
      else onleave();
    } else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<div class="search">
  <input
    class="query"
    type="search"
    spellcheck="false"
    autocomplete="off"
    placeholder="Search requests: text, repo:, eval:, kind:, status:, is:mine …"
    aria-label="Search requests"
    role="combobox"
    aria-autocomplete="list"
    aria-expanded={open}
    aria-controls="search-suggestions"
    aria-activedescendant={open && q.active >= 0 ? `search-suggestion-${q.active}` : undefined}
    bind:value
    bind:this={input}
    // Whether a term is still being typed (a space inside a quoted value does not end it) is
    // the library's answer: its completion keeps the whole query when none is.
    oninput={() => (value === "" ? close() : void suggest())}
    onkeydown={keydown}
    onblur={close}
  />
  {#if open && q.completion}
    <ul class="suggestions" id="search-suggestions" role="listbox" aria-label="Suggestions">
      {#each q.completion.candidates as candidate, at (candidate)}
        <li
          id="search-suggestion-{at}"
          role="option"
          aria-selected={at === q.active}
          class:active={at === q.active}
          onmousedown={(event) => {
            // Keep focus in the field; the click takes the suggestion.
            event.preventDefault();
            accept(candidate);
          }}
        >
          {candidate}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .search {
    position: relative;
    padding: 0 var(--pad) 6px;
  }
  .query {
    width: 100%;
    font: 12px var(--mono);
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
  }
  .query:focus {
    outline: none;
    border-color: var(--accent);
  }
  .suggestions {
    position: absolute;
    z-index: 10;
    left: var(--pad);
    right: var(--pad);
    margin: 2px 0 0;
    padding: 4px 0;
    list-style: none;
    max-height: 240px;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 6px 18px rgb(0 0 0 / 20%);
  }
  .suggestions li {
    padding: 4px 10px;
    font: 12px var(--mono);
    cursor: pointer;
  }
  .suggestions li:hover,
  .suggestions .active {
    background: var(--selection);
  }
</style>
