<script lang="ts">
  // The graph's find field, opened with Ctrl+F: the library lists the Artifacts and evals of the
  // shown Run or Current whose names contain the typed text, in the graph's order, an eval as
  // `{Artifact} / {eval}`. ↑/↓ choose one, Enter or a click opens it; Esc gives focus back to
  // where it was and closes the field, as leaving it does. Keys mid-composition belong to the
  // input method.
  import type { Attachment } from "svelte/attachments";
  import { call, message, useIpc, type Found, type Source } from "../ipc/client";
  import { GraphFindState, setGraphFindState } from "./graph-find-state.svelte";
  import { composing } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    source,
    onchoose,
    onclose,
  }: {
    source: Source;
    /** Open what was found. */
    onchoose: (found: Found) => void;
    onclose: () => void;
  } = $props();
  const commands = useIpc();
  const f = new GraphFindState();
  setGraphFindState(f);

  /** The answer for the text as typed now, if it has come. */
  const finding = $derived(f.found !== null && f.found.text === f.text ? f.found.finding : null);
  const found = $derived(finding?.found ?? []);

  /** The field takes focus as it opens, noting what had it. */
  const take: Attachment<HTMLInputElement> = (input) => {
    f.before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    input.focus();
  };

  /** Ask the library for the text as typed; a slower answer to an older text never shows. */
  function find() {
    const [text, asked] = [f.text, ++f.asked];
    f.active = -1;
    if (text.trim() === "") return void (f.found = null);
    call(commands.find(source, text)).then(
      (answer) => {
        if (asked !== f.asked) return;
        f.found = { text, finding: answer };
        f.error = null;
      },
      (failure) => {
        if (asked === f.asked) f.error = message(failure);
      },
    );
  }

  function keydown(event: KeyboardEvent) {
    if (composing(event)) return;
    if (event.key === "ArrowDown" && found.length > 0) f.active = (f.active + 1) % found.length;
    else if (event.key === "ArrowUp" && found.length > 0) f.active = f.active <= 0 ? found.length - 1 : f.active - 1;
    else if (event.key === "Enter" && found.length > 0) onchoose(found[Math.max(f.active, 0)]);
    else if (event.key === "Escape") {
      f.before?.focus();
      onclose();
    } else if (event.key !== "Enter") return;
    event.preventDefault();
    event.stopPropagation();
  }

  /** How an entry reads: an Artifact by its whole name, an eval after its Artifact's. */
  const named = (item: Found) => (item.eval === null ? item.artifact : `${item.artifact} / ${item.eval}`);
</script>

<div class="find">
  <input
    class="query"
    type="search"
    spellcheck="false"
    autocomplete="off"
    placeholder="Find an Artifact or an eval"
    aria-label="Find in the graph"
    role="combobox"
    aria-autocomplete="list"
    aria-expanded={found.length > 0}
    aria-controls="graph-found"
    aria-activedescendant={f.active >= 0 ? `graph-found-${f.active}` : undefined}
    bind:value={f.text}
    {@attach take}
    oninput={find}
    onkeydown={keydown}
    onblur={onclose}
  />
  {#if f.error}
    <p class="note error">{f.error}</p>
  {:else if finding}
    {#if found.length > 0}
      <ul class="found" id="graph-found" role="listbox" aria-label="Found">
        {#each found as item, at (`${item.node}\n${item.tab}`)}
          <li
            id="graph-found-{at}"
            role="option"
            aria-selected={at === f.active}
            class:active={at === f.active}
            onmousedown={(event) => {
              // Keep focus in the field; the click opens what was found.
              event.preventDefault();
              onchoose(item);
            }}
          >
            <span class="glyph {tone(item.tone)}">{item.glyph}</span><span class="name">{named(item)}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="note" role="status">
      {#if found.length === 0}No Artifact or eval matches.{:else if finding.more > 0}{finding.more} more match.{/if}
    </p>
  {/if}
</div>

<style>
  .find {
    position: absolute;
    z-index: 10;
    left: 8px;
    top: 8px;
    width: min(380px, calc(100% - 16px));
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 6px 18px rgb(0 0 0 / 20%);
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
  .found {
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    max-height: 320px;
    overflow: auto;
  }
  .found li {
    display: flex;
    gap: 6px;
    padding: 4px 8px;
    font: 12px var(--mono);
    border-radius: 4px;
    cursor: pointer;
  }
  .found li:hover,
  .found .active {
    background: var(--selection);
  }
  .name {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .note {
    margin: 4px 8px 0;
    color: var(--muted);
    font-size: 12px;
  }
  .note:empty {
    display: none;
  }
  .note.error {
    color: var(--red);
  }
</style>
