<script lang="ts">
  // The chosen verdict's owner fields: typed inputs for a flat schema, JSON text otherwise.
  // Enter never submits; Ctrl+S does. Values are checked and coerced by the backend's form.
  import type { Attachment } from "svelte/attachments";
  import type { FieldValue } from "../ipc/client";
  import type { FormDraft, Review } from "./review.svelte";
  import { composing, label } from "../shell/keys";

  let { review, form }: { review: Review; form: FormDraft } = $props();

  /** Typed fields or JSON text: switching either way moves focus like opening a form does. */
  const mode = $derived(form.json === null ? "fields" : "json");

  /** Typing goes to the form: the first editable field takes focus when a verdict opens or
   * its typed fields become JSON. */
  const takeFocus: Attachment<HTMLElement> = (root) => {
    void form;
    void mode;
    queueMicrotask(() => root.querySelector<HTMLElement>("input:not([readonly]), textarea, select")?.focus());
  };

  function set(name: string, value: FieldValue) {
    void form.set(name, value);
  }

  /** Tab indents JSON, as in the terminal; Shift+Tab still leaves the field. */
  function indent(event: KeyboardEvent) {
    if (event.key !== "Tab" || event.shiftKey || composing(event)) return;
    event.preventDefault();
    const area = event.currentTarget as HTMLTextAreaElement;
    area.setRangeText("  ", area.selectionStart, area.selectionEnd, "end");
    void form.setJson(area.value);
  }

  const text = (value: FieldValue) => ("value" in value && typeof value.value === "string" ? value.value : "");
</script>

<div class="form" {@attach takeFocus}>
  <div class="verdicts" role="radiogroup" aria-label="Verdict">
    {#each ["GREEN", "RED"] as const as verdict (verdict)}
      <button
        role="radio"
        aria-checked={form.verdict === verdict}
        class="verdict {verdict.toLowerCase()}"
        onclick={() => void review.choose(verdict)}
      >
        {verdict} <kbd>{label(verdict === "GREEN" ? "Ctrl+G" : "Ctrl+R")}</kbd>
      </button>
    {/each}
    {#if form.json === null}
      <button class="verdict json" onclick={() => void review.editJson()}>
        Edit as JSON <kbd>{label("Ctrl+E")}</kbd>
      </button>
    {/if}
  </div>

  {#if form.json !== null}
    <label class="field">
      <span class="name">Owner fields (JSON)</span>
      <textarea
        class="json"
        spellcheck="false"
        value={form.json}
        oninput={(event) => void form.setJson((event.currentTarget as HTMLTextAreaElement).value)}
        onkeydown={indent}
        rows="12"
      ></textarea>
    </label>
  {:else if form.fields.length === 0}
    <p class="muted">This verdict has no owner fields. Submit with {label("Ctrl+S")}.</p>
  {:else}
    {#each form.fields as field (field.name)}
      {@const value = form.values[field.name]}
      <div class="field">
        <label class="name" for="field-{field.name}">
          {field.name}{#if field.required}<span class="required" title="required">*</span>{/if}
        </label>
        {#if field.input.kind === "fixed"}
          <input id="field-{field.name}" readonly value="{field.input.value} (fixed)" />
        {:else if field.input.kind === "bool"}
          {@const chosen = value.kind === "bool" ? value.value : null}
          <div class="choices" role="radiogroup" aria-labelledby="field-{field.name}">
            {#each [true, false] as option (option)}
              <label>
                <input
                  type="radio"
                  name="field-{field.name}"
                  checked={chosen === option}
                  onchange={() => set(field.name, { kind: "bool", value: option })}
                />
                {option}
              </label>
            {/each}
            {#if !field.required}
              <label>
                <input
                  type="radio"
                  name="field-{field.name}"
                  checked={chosen === null}
                  onchange={() => set(field.name, { kind: "bool", value: null })}
                />
                –
              </label>
            {/if}
          </div>
        {:else if field.input.kind === "choice"}
          {@const chosen = value.kind === "choice" ? value.value : null}
          <select
            id="field-{field.name}"
            value={chosen === null ? "" : String(chosen)}
            onchange={(event) => {
              const raw = (event.currentTarget as HTMLSelectElement).value;
              set(field.name, { kind: "choice", value: raw === "" ? null : Number(raw) });
            }}
          >
            <option value="">–</option>
            {#each field.input.options as option, at (at)}
              <option value={String(at)}>{option}</option>
            {/each}
          </select>
        {:else}
          {@const kind = field.input.kind}
          {#if kind === "text"}
            <textarea
              id="field-{field.name}"
              rows="2"
              value={text(value)}
              oninput={(event) => set(field.name, { kind, value: (event.currentTarget as HTMLTextAreaElement).value })}
            ></textarea>
          {:else}
            <input
              id="field-{field.name}"
              inputmode={kind === "integer" ? "numeric" : "decimal"}
              value={text(value)}
              oninput={(event) => set(field.name, { kind, value: (event.currentTarget as HTMLInputElement).value })}
            />
          {/if}
        {/if}
        {#if field.hint}<span class="hint">{field.hint}</span>{/if}
      </div>
    {/each}
  {/if}

  {#if form.error}
    <div class="error" role="alert">{form.error}</div>
  {/if}
</div>

<style>
  .form {
    display: grid;
    gap: 12px;
  }
  .verdicts {
    display: flex;
    gap: 6px;
  }
  .verdict {
    font: inherit;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--muted);
    border-radius: 6px;
    padding: 3px 12px;
    cursor: pointer;
  }
  .verdict.json {
    margin-left: auto;
  }
  .verdict.green[aria-checked="true"] {
    color: var(--green);
    border-color: var(--green);
    font-weight: 600;
  }
  .verdict.red[aria-checked="true"] {
    color: var(--red);
    border-color: var(--red);
    font-weight: 600;
  }
  .field {
    display: grid;
    gap: 4px;
  }
  .name {
    font-weight: 600;
  }
  .required {
    color: var(--red);
    margin-left: 2px;
  }
  .hint,
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  input,
  textarea,
  select {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 8px;
    width: 100%;
  }
  textarea {
    resize: vertical;
  }
  .json {
    font: 12px/1.5 var(--mono);
  }
  input:focus,
  textarea:focus,
  select:focus {
    outline: 2px solid var(--focus);
    outline-offset: -1px;
  }
  .choices {
    display: flex;
    gap: 16px;
  }
  .choices input {
    width: auto;
  }
  .error {
    color: var(--red);
    white-space: pre-wrap;
    border: 1px solid var(--red);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
  }
</style>
