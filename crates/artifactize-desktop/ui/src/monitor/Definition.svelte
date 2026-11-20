<script lang="ts">
  // An Artifact's definition on a Current, in the Detail where a Run's shows its Path: the file
  // that defines it; its Path, a field written by Enter and put back by Esc, a missing target
  // said beside it; its Parts, each removed by its button and one added by name from what the
  // field suggests; then Comment (its field opens under its node on the graph), Rename and
  // Delete (their dialogs). Source follows, folded until unfolded: the file's text, typed over
  // in place, written by Save or Ctrl+S and put back by Revert. Every edit names the text it was
  // shown; a refusal is said under what it concerns, and what was typed stays.
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import Section from "../shell/Section.svelte";
  import { composing } from "../shell/keys";
  import type { Place } from "./definitions.svelte";

  let { branch, name }: { branch: BranchKey; name: string } = $props();
  const { definitions: defs } = getAppState();

  const node = $derived(defs.node(name));
  const file = $derived(node ? (defs.view?.files[node.definition] ?? "") : "");
  const draft = $derived(defs.drafts.get(name));
  /** What the Source shows: what was typed, else the file's text. */
  const source = $derived(draft?.text ?? file);
  const lines = $derived(source.split("\n").length);

  /** What is typed here: the Path, a part being added, and why the last edit wrote nothing. */
  const form = $derived(defs.shownForm(name));
  const { path, adding, chosen, refused } = $derived(form);

  /** The line a refused Source names, marked in its margin. */
  const marked = $derived(refused?.at === "source" ? Number(/line (\d+)/.exec(refused.text)?.[1] ?? 0) : 0);

  /** The project's Artifacts the Parts field suggests: not this one, not its parts, those whose
   * names hold what is typed. */
  const suggested = $derived.by(() => {
    if (adding === null || !defs.view || !node) return [];
    const typed = adding.trim().toLowerCase();
    return defs.view.nodes
      .map((other) => other.name)
      .filter((other) => other !== name && !node.parts.includes(other) && other.toLowerCase().includes(typed))
      .slice(0, 8);
  });

  async function edit(at: Place, operation: Parameters<typeof defs.edit>[1]): Promise<boolean> {
    const typing = defs.form(name);
    typing.refused = null;
    const why = await defs.edit(branch, operation, "detail");
    if (why !== null) typing.refused = { at, text: why };
    return why === null;
  }

  function pathKey(event: KeyboardEvent) {
    if (composing(event)) return;
    if (alone(event, "Enter") && path !== null) {
      event.preventDefault();
      const wanted = path;
      const typing = defs.form(name);
      if (wanted === node?.target) typing.path = null;
      else void edit("path", { kind: "setPath", name, path: wanted }).then((written) => written && (typing.path = null));
    } else if (alone(event, "Escape") && path !== null) {
      event.preventDefault();
      event.stopPropagation();
      const typing = defs.form(name);
      typing.path = null;
      if (typing.refused?.at === "path") typing.refused = null;
    }
  }

  function partKey(event: KeyboardEvent) {
    if (composing(event) || adding === null) return;
    const typing = defs.form(name);
    if (alone(event, "ArrowDown") || alone(event, "ArrowUp")) {
      event.preventDefault();
      if (suggested.length > 0) typing.chosen = (chosen + (event.key === "ArrowDown" ? 1 : suggested.length - 1)) % suggested.length;
    } else if (alone(event, "Enter")) {
      event.preventDefault();
      addPart(suggested[chosen] ?? adding.trim());
    } else if (alone(event, "Escape")) {
      event.preventDefault();
      event.stopPropagation();
      typing.adding = null;
      if (typing.refused?.at === "parts") typing.refused = null;
    }
  }

  function addPart(part: string) {
    if (part) void edit("parts", { kind: "addPart", whole: name, part, at: null }).then((written) => written && (defs.form(name).adding = null));
  }

  function type(text: string) {
    if (text === file) defs.drafts.delete(name);
    else defs.drafts.set(name, { base: draft?.base ?? file, text });
  }

  async function save() {
    if (!draft) return;
    await edit("source", { kind: "saveSource", name, source: draft.text });
  }

  /** Put back the file's text as it is on disk now. */
  async function revert() {
    await defs.load(branch);
    defs.drafts.delete(name);
    if (refused?.at === "source") defs.form(name).refused = null;
  }

  function sourceKey(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      event.stopPropagation();
      void save();
    }
  }

  const grow = (field: HTMLElement) => field.focus();

  /** `name` pressed alone in a field: no Ctrl, ⌘ or Alt, no composition. */
  const alone = (event: KeyboardEvent, name: string) => event.key === name && !event.ctrlKey && !event.metaKey && !event.altKey && !composing(event);
</script>

{#if node}
  <Section name="Definition">
    <dl class="facts">
      <dt>File</dt>
      <dd class="file">{node.definition}</dd>
      <dt>Path</dt>
      <dd>
        <input
          class="field-input"
          aria-label="Path of {name}"
          spellcheck="false"
          autocomplete="off"
          value={path ?? node.target}
          oninput={(event) => (defs.form(name).path = event.currentTarget.value)}
          onkeydown={pathKey}
        />
        {#if node.targetMissing}<p class="refusal">{node.target} does not exist.</p>{/if}
        {#if refused?.at === "path"}<p class="refusal" role="alert">{refused.text}</p>{/if}
      </dd>
      <dt>Parts</dt>
      <dd>
        <ul class="parts" aria-label="Parts of {name}">
          {#each node.parts as part (part)}
            <li>
              <span class="part">{part}</span>
              <button class="remove" aria-label="Remove {part} from the parts of {name}" onclick={() => void edit("parts", { kind: "removePart", whole: name, part })}><Icon name="remove" /></button>
            </li>
          {/each}
        </ul>
        {#if adding === null}
          <button class="button small quiet add" onclick={() => Object.assign(defs.form(name), { adding: "", chosen: 0 })}><Icon name="add" />Add</button>
        {:else}
          <input
            class="field-input"
            aria-label="Add a part to {name}"
            role="combobox"
            aria-expanded={suggested.length > 0}
            aria-controls="parts-of-{name}"
            spellcheck="false"
            autocomplete="off"
            value={adding}
            oninput={(event) => Object.assign(defs.form(name), { adding: event.currentTarget.value, chosen: 0 })}
            onkeydown={partKey}
            onblur={() => adding === "" && (defs.form(name).adding = null)}
            {@attach grow}
          />
          {#if suggested.length > 0}
            <ul class="suggested" id="parts-of-{name}" role="listbox">
              {#each suggested as other, at (other)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <li role="option" aria-selected={at === chosen} onmousedown={(event) => event.preventDefault()} onclick={() => addPart(other)}>{other}</li>
              {/each}
            </ul>
          {/if}
        {/if}
        {#if refused?.at === "parts"}<p class="refusal" role="alert">{refused.text}</p>{/if}
      </dd>
    </dl>
    <div class="actions">
      <button class="button small quiet" data-comment-button onclick={() => defs.openField(name, "button")}>Comment</button>
      <button class="button small quiet" onclick={() => (defs.dialog = { kind: "rename", name })}>Rename</button>
      <button class="button small quiet" onclick={() => (defs.dialog = { kind: "delete", name })}>Delete</button>
    </div>
  </Section>
  <Section name="Source">
    <div class="source">
      {#if marked > 0}<div class="mark" aria-hidden="true" style:top="calc(6px + {marked - 1} * var(--text-small) * 1.5)"></div>{/if}
      <ol class="margin" aria-hidden="true">
        {#each { length: lines } as _, at (at)}<li class:marked={at + 1 === marked}>{at + 1}</li>{/each}
      </ol>
      <textarea
        class="text"
        aria-label="Source of {name}"
        spellcheck="false"
        autocomplete="off"
        wrap="off"
        rows={lines}
        value={source}
        oninput={(event) => type(event.currentTarget.value)}
        onkeydown={sourceKey}
      ></textarea>
    </div>
    {#if refused?.at === "source"}<p class="refusal" role="alert">{refused.text}</p>{/if}
    {#if draft}
      <div class="actions">
        <button class="button small primary" disabled={defs.busy} onclick={() => void save()}>Save</button>
        <button class="button small" onclick={() => void revert()}>Revert</button>
      </div>
    {/if}
  </Section>
{:else if defs.error}
  <p class="refusal">{defs.error}</p>
{/if}

<style>
  .facts {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 12px;
    margin: 0;
    padding: 4px 0 0;
    align-items: baseline;
  }
  dt {
    color: var(--ink-2);
    text-align: right;
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  .file {
    font: var(--text-small) var(--mono);
    color: var(--ink-2);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .parts {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .parts li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--row-dense);
  }
  .remove {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .remove:hover {
    color: var(--ink);
    background: var(--surface-group);
  }
  .add {
    margin-top: 2px;
  }
  .suggested {
    margin: 2px 0 0;
    padding: 2px 0;
    list-style: none;
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    background: var(--surface-raised);
    font: var(--text-small) var(--mono);
  }
  .suggested li {
    padding: 2px 8px;
    min-height: var(--row-dense);
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .suggested li:not([aria-selected="true"]):hover {
    background: var(--surface-group);
  }
  /* The chosen suggestion, with the inset bar while the keyboard is in the field. */
  .suggested li[aria-selected="true"] {
    background: var(--selection);
  }
  dd:focus-within .suggested li[aria-selected="true"] {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .actions {
    display: flex;
    gap: var(--gap);
    padding: 6px 0 0;
  }
  .refusal {
    margin: 4px 0 0;
    color: var(--red);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  /* The file's text in monospace on a field's surface, its line numbers in faint ink; it scrolls
     sideways in its own box rather than wrap, so each number stays with its line. */
  .source {
    position: relative;
    display: flex;
    margin: 4px 0 0;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    font: var(--text-small) / 1.5 var(--mono);
    overflow: hidden;
  }
  .margin {
    flex: none;
    margin: 0;
    padding: 6px 8px 6px 0;
    list-style: none;
    color: var(--ink-3);
    text-align: right;
    min-width: 28px;
    user-select: none;
  }
  .margin li.marked {
    color: var(--ink);
  }
  /* A refused line: the selection's background across its number and its text. */
  .mark {
    position: absolute;
    left: 0;
    right: 0;
    height: calc(var(--text-small) * 1.5);
    background: var(--selection);
    pointer-events: none;
  }
  .margin,
  .text {
    position: relative;
  }
  .text {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 6px 8px 6px 0;
    border: 0;
    background: none;
    color: var(--ink);
    font: inherit;
    line-height: inherit;
    resize: none;
    white-space: pre;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .text:focus-visible {
    outline: none;
  }
  .source:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: -1px;
  }
</style>
