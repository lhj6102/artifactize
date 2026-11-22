<script lang="ts">
  // The chosen verdict's owner fields as a person enters them: a text, a choice, a list of items,
  // or repeated groups of fields, each to add and remove. Constants are filled in and not shown.
  // Enter never submits; the Submit button of the review's action row and Ctrl+S do. Values are
  // checked and coerced by the backend's form.
  import type { Attachment } from "svelte/attachments";
  import type { FieldValue, FormField } from "../ipc/client";
  import type { FormDraft, Review } from "./review.svelte";
  import type { FormCaret } from "./form-caret";
  import VerdictChoices from "./VerdictChoices.svelte";
  import Icon from "../shell/Icon.svelte";

  let { review, form, caret, showError = true }: { review: Review; form: FormDraft; caret: FormCaret; showError?: boolean } = $props();
  /** What a person enters: constants are the form's own. */
  const entered = (fields: FormField[]) => fields.filter((field) => field.input.kind !== "fixed");
  const shown = $derived(entered(form.fields));

  /** Typing goes to the form: the first field takes focus when a verdict opens. */
  const takeFocus: Attachment<HTMLElement> = (root) => {
    void form;
    queueMicrotask(() => root.querySelector<HTMLElement>("input, textarea, select")?.focus());
  };

  function set(name: string, value: FieldValue) {
    void form.set(name, value);
  }

  const text = (value: FieldValue | undefined) => (value && "value" in value && typeof value.value === "string" ? value.value : "");
  /** An optional field says so in grey inside its empty input. */
  const placeholder = (required: boolean) => (required ? undefined : "Optional");
  const items = (value: FieldValue | undefined) => (value?.kind === "list" ? value.value : []);
  /** A list with one item edited; adding and removing items is the library's. */
  function setItems(name: string, next: string[]) {
    set(name, { kind: "list", value: next });
  }

  type Group = [string, FieldValue][];
  const groups = (value: FieldValue | undefined): Group[] => (value?.kind === "groups" ? value.value : []);
  /** The groups with one group's field edited. */
  function setGroups(name: string, next: Group[]) {
    set(name, { kind: "groups", value: next });
  }
  /** `group` with field `name` set to `value`. */
  const within = (group: Group, name: string, value: FieldValue): Group => {
    const at = group.findIndex(([sub]) => sub === name);
    return at < 0 ? [...group, [name, value]] : group.with(at, [name, value]);
  };
  const valueIn = (group: Group, name: string) => group.find(([sub]) => sub === name)?.[1];
</script>

<!-- One scalar input: `id` names it for its label, `label` for screen readers, `onset` takes
     the new value. -->
{#snippet scalar(field: FormField, value: FieldValue | undefined, id: string, onset: (value: FieldValue) => void, comment = false)}
  {#if field.input.kind === "bool"}
    {@const chosen = value?.kind === "bool" ? value.value : null}
    <div class="choices" role="radiogroup" aria-labelledby={id}>
      {#each [true, false] as option (option)}
        <label>
          <input type="radio" name={id} checked={chosen === option} onchange={() => onset({ kind: "bool", value: option })} />
          {option}
        </label>
      {/each}
      {#if !field.required}
        <label>
          <input type="radio" name={id} checked={chosen === null} onchange={() => onset({ kind: "bool", value: null })} />
          <span class="optional">Optional</span>
        </label>
      {/if}
    </div>
  {:else if field.input.kind === "choice"}
    {@const chosen = value?.kind === "choice" ? value.value : null}
    <div class="choice"><select
      {id}
      class:empty={chosen === null}
      value={chosen === null ? "" : String(chosen)}
      onchange={(event) => {
        const raw = (event.currentTarget as HTMLSelectElement).value;
        onset({ kind: "choice", value: raw === "" ? null : Number(raw) });
      }}
    >
      <option value="">{field.required ? "–" : "Optional"}</option>
      {#each field.input.options as option, at (at)}
        <option value={String(at)}>{option}</option>
      {/each}
    </select><span><Icon name="unfold" /></span></div>
  {:else if field.input.kind === "text"}
    <textarea
      {id}
      data-comment={comment || undefined}
      rows="2"
      placeholder={placeholder(field.required)}
      value={text(value)}
      oninput={(event) => onset({ kind: "text", value: (event.currentTarget as HTMLTextAreaElement).value })}
    ></textarea>
  {:else if field.input.kind === "integer" || field.input.kind === "number"}
    {@const kind = field.input.kind}
    <input
      {id}
      class="number"
      placeholder={placeholder(field.required)}
      inputmode={kind === "integer" ? "numeric" : "decimal"}
      value={text(value)}
      oninput={(event) => onset({ kind, value: (event.currentTarget as HTMLInputElement).value })}
    />
  {/if}
{/snippet}

<div class="form" {@attach caret.attach} {@attach takeFocus}>
  <VerdictChoices {review} selected={form.verdict} />

  {#each shown as field (field.name)}
      {@const value = form.values[field.name]}
      <div class="field">
        <label class="name" for="field-{field.name}">
          {field.name}{#if field.required}<span class="required" aria-label="required">*</span>{/if}
        </label>
        {#if field.input.kind === "list"}
          {@const list = items(value)}
          <ol class="items" id="field-{field.name}">
            {#each list as item, at (at)}
              <li>
                <textarea
                  rows="2"
                  aria-label="{field.name} {at + 1}"
                  placeholder={placeholder(field.required)}
                  value={item}
                  oninput={(event) =>
                    setItems(field.name, list.with(at, (event.currentTarget as HTMLTextAreaElement).value))}
                ></textarea>
                <button class="remove" aria-label="Remove {field.name} {at + 1}" onclick={() => void form.remove(field.name, at)}><Icon name="remove" /></button>
              </li>
            {/each}
          </ol>
          <button class="button small add" onclick={() => void form.add(field.name)}><Icon name="add" />Add</button>
          {#if list.length === 0 && !field.required}<span class="optional">Optional</span>{/if}
        {:else if field.input.kind === "groups"}
          {@const subs = entered(field.input.fields)}
          {@const list = groups(value)}
          <div class="groups" id="field-{field.name}" role="group" aria-label={field.name}>
            {#each list as group, at (at)}
              <div class="group" role="group" aria-label="{field.name} {at + 1}">
                <div class="group-head">
                  <span>{field.name} {at + 1}</span>
                  <button class="remove" aria-label="Remove {field.name} {at + 1}" onclick={() => void form.remove(field.name, at)}><Icon name="remove" /></button>
                </div>
                {#each subs as sub (sub.name)}
                  {@const id = `field-${field.name}-${at}-${sub.name}`}
                  <div class="field">
                    <label class="name" for={id} id={sub.input.kind === "bool" ? id : undefined}>
                      {sub.name}{#if sub.required}<span class="required" aria-label="required">*</span>{/if}
                    </label>
                    {@render scalar(sub, valueIn(group, sub.name), id, (next) => setGroups(field.name, list.with(at, within(group, sub.name, next))))}
                    {#if sub.hint}<span class="hint">{sub.hint}</span>{/if}
                  </div>
                {/each}
              </div>
            {/each}
          </div>
          <button class="button small add" onclick={() => void form.add(field.name)}><Icon name="add" />Add</button>
        {:else}
          {@render scalar(field, value, `field-${field.name}`, (next) => set(field.name, next), field.name === "comment")}
        {/if}
        {#if field.hint}<span class="hint">{field.hint}</span>{/if}
      </div>
  {/each}

  {#if showError && form.error}
    <div class="error" role="alert">{form.error}</div>
  {/if}
</div>

<style>
  .form {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    overflow-wrap: anywhere;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
  }
  .name {
    color: var(--label);
  }
  .required {
    color: var(--red);
    margin-left: 2px;
  }
  .hint {
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  /* Fields sit one step below the panel, outlined by the faintest line. */
  input:not([type="radio"]),
  textarea,
  select {
    font: inherit;
    color: var(--ink);
    background: var(--surface-input);
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    height: var(--control);
    padding: 0 8px;
    width: 100%;
  }
  textarea {
    height: auto;
    min-height: 44px;
    padding: 4px 8px;
    field-sizing: content;
    min-height: calc(2lh + 10px);
    resize: none;
  }
  /* An empty choice reads as a placeholder does. */
  select.empty {
    color: var(--ink-2);
  }
  select.empty option {
    color: var(--ink);
  }
  ::placeholder {
    color: var(--ink-2);
  }
  /* A number takes a short one-line field. */
  .number {
    width: 120px;
  }
  /* A list of text is numbered items, each its field with the remove icon at its right. */
  .items {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
    counter-reset: item;
  }
  .items li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: start;
    gap: 6px;
    counter-increment: item;
  }
  .items li::before {
    content: counter(item) ".";
    min-width: 16px;
    line-height: var(--control);
    color: var(--ink-2);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .remove {
    display: inline-grid;
    place-items: center;
    width: var(--row-dense);
    height: var(--row-dense);
    border: 0;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
    font: inherit;
    padding: 0;
  }
  .remove:hover {
    background: var(--surface-group);
    border-radius: var(--radius-control);
    color: var(--ink);
  }
  .add {
    justify-self: start;
  }
  .groups {
    display: grid;
    gap: 8px;
  }
  .group {
    display: grid;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
  }
  .choices label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .choice {
    position: relative;
  }
  .choice select {
    appearance: none;
    padding-inline-end: 24px;
  }
  .choice span {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    color: var(--ink-2);
    pointer-events: none;
  }
  .error {
    color: var(--red);
    white-space: pre-wrap;
    font-size: var(--text-small);
  }
  /* An empty optional list says so in grey beside its Add (J-HUMAN-FORM). */
  .optional {
    margin-left: var(--gap);
    color: var(--ink-2);
    font-size: var(--text-small);
  }
</style>
