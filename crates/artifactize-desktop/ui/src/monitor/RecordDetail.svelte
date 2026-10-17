<script lang="ts">
  // The read-only Detail of a request that is not an open Human review. A request with a log
  // switches between its Summary and its log: an Agent's live transcript, or a runtime's saved
  // output. An eval's Summary is its status (with what running it cost, or what reuse saved),
  // its instruction and its result, then its execution options, metadata and fingerprint; every
  // other fact folds under Debug. The monitor's opened Detail and the review screen's Detail of
  // any other request both show it.
  import type { DetailView, Field } from "../ipc/client";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import { tone } from "../shell/tone";

  let {
    detail,
    technical = $bindable(false),
    pending = $bindable(false),
    log = $bindable(false),
    debug = $bindable(false),
  }: {
    detail: DetailView;
    technical?: boolean;
    pending?: boolean;
    /** The log rather than the Summary. */
    log?: boolean;
    /** Debug unfolded. */
    debug?: boolean;
  } = $props();

  const agent = $derived(detail.profile === "agent" && detail.request !== null);
  /** The name of the request's log, if it has one. */
  const logName = $derived(agent ? "Agent log" : detail.evidence ? "Runtime log" : null);
  const showLog = $derived(log && logName !== null);
  const card = $derived(detail.card);
  /** An Artifact's Detail: its status line, path and what it waits for; the rest under Debug. */
  const artifact = $derived(detail.node.startsWith("a:"));

  /** The Summary's own blocks, each named by the Detail fields it shows. */
  const OPTIONS = ["Profile", "Options"];
  const SHOWN = new Set(["Status", "Instruction", "Result", "Error", "Fingerprint", ...OPTIONS]);
  /** An Artifact's evals are listed above its Detail already. */
  const ARTIFACT_SHOWN = new Set(["Status", "Path", "Evals"]);

  const fields = $derived(detail.sections.flatMap((section) => section.fields));
  const waits = $derived(detail.sections.flatMap((section) => section.waits));
  const pick = (keys: string[]): Field[] => fields.filter((field) => keys.includes(field.key));
  const debugFields = $derived(fields.filter((field) => !(artifact ? ARTIFACT_SHOWN : SHOWN).has(field.key)));

  /** Tokens in thousands or millions: `48.2k`, `1.3M` (a lone `m` would read as minutes). */
  function count(tokens: number): string {
    if (tokens < 1000) return String(tokens);
    if (tokens < 1_000_000) return `${(tokens / 1000).toFixed(1).replace(/\.0$/, "")}k`;
    return `${(tokens / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  }
  /** What running it took and spent, or what reuse saved. A Human eval has none. */
  const spent = $derived.by(() => {
    if (!card || detail.profile === "human") return [];
    const saved = card.reused ? "saved " : "";
    return [card.took && `${saved}${card.took}`, card.tokens !== null && `${saved}${count(card.tokens)} tokens`].filter(
      (part): part is string => Boolean(part),
    );
  });
  /** Grey beside the status: the time, tokens and model. */
  const cost = $derived([...spent, ...(card?.model && detail.profile !== "human" ? [card.model] : [])].join(" · "));
  const status = $derived(detail.mark ? `${detail.mark.glyph} ${detail.mark.status}${card?.reused ? " · REUSED" : ""}` : "");
  const hasResult = $derived(card !== null && (card.result !== null || card.error !== null || card.exitCode !== null));
</script>

<!-- Focusable so the arrows and PgUp/PgDn scroll the Summary. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="full" tabindex="0" role="document">
  {#if logName}
    <div class="views" role="tablist" aria-label="Views">
      <button class="view" role="tab" aria-selected={!showLog} onclick={() => (log = false)}>Summary</button>
      <span class="bar" aria-hidden="true">|</span>
      <button class="view" role="tab" aria-selected={showLog} onclick={() => (log = true)}>{logName}</button>
    </div>
  {/if}
  {#if card}
    <p class="summary">
      <span class="status {tone(detail.mark?.status)}">{status}</span>
      {#if cost}<span class="cost">{cost}</span>{/if}
    </p>
  {:else if detail.summary}
    <p class="summary {tone(detail.mark?.status)}">{detail.summary}</p>
  {/if}
  {#if showLog && agent && detail.request}
    <div class="shown">
      {#key detail.request}
        <Transcript id={detail.request} />
      {/key}
    </div>
  {:else if showLog && detail.evidence}
    <div class="shown evidence">
      <h3>{detail.evidence.title}</h3>
      <pre>{detail.evidence.text}</pre>
    </div>
  {:else if artifact}
    <div class="shown card">
      {#each pick(["Path"]) as field (field.key)}<p class="line muted">{field.value}</p>{/each}
      {#if waits.length > 0}
        <section class="block">
          <h3>Waits for</h3>
          {#each waits.filter((line) => !line.pending) as line, at (at)}<p class="line">{line.text}</p>{/each}
        </section>
      {/if}
      {#if debugFields.length > 0}
        <section class="block">
          <button class="fold" aria-expanded={debug} onclick={() => (debug = !debug)}>{debug ? "▾" : "▸"} Debug</button>
          {#if debug}
            <dl>
              {#each debugFields as field, at (at)}
                <dt>{field.key}</dt>
                <dd>{field.value}</dd>
              {/each}
            </dl>
          {/if}
        </section>
      {/if}
    </div>
  {:else if card}
    <div class="shown card">
      {#if card.instruction}<p class="instruction">{card.instruction}</p>{/if}
      {#if hasResult}
        <section class="block result">
          <h3>Result</h3>
          {#if card.error}<p class="error">{card.error}</p>{/if}
          {#if card.exitCode !== null}<p>exit {card.exitCode}</p>{/if}
          {#each card.result ?? [] as entry (entry.key)}
            {#if entry.text !== null}
              <p class="text">{#if entry.key !== "summary"}<span class="key">{entry.key}</span> {/if}{entry.text}</p>
            {:else}
              <p class="count">{entry.items.length} {entry.key}</p>
              <ol class="items">
                {#each entry.items as item, at (at)}
                  <li>
                    {#if item.head}<p class="head">{item.head}</p>{/if}
                    {#each item.fields as field, part (part)}
                      <p class="line">{#if item.fields.length > 1}<span class="key">{field.key}</span> {/if}{field.value}</p>
                    {/each}
                  </li>
                {/each}
              </ol>
            {/if}
          {/each}
        </section>
      {/if}
      {#each [["Execution options", pick(OPTIONS).map((field) => field.value)], ["Execution metadata", spent.length > 0 ? [spent.join(" · ")] : []], ["Fingerprint", pick(["Fingerprint"]).map((field) => field.value)]] as const as [title, lines] (title)}
        {#if lines.length > 0}
          <section class="block">
            <h3>{title}</h3>
            {#each lines as line, at (at)}<p class="line">{line}</p>{/each}
          </section>
        {/if}
      {/each}
      {#if debugFields.length > 0 || waits.length > 0}
        <section class="block">
          <button class="fold" aria-expanded={debug} onclick={() => (debug = !debug)}>{debug ? "▾" : "▸"} Debug</button>
          {#if debug}
            <dl>
              {#each debugFields as field, at (at)}
                <dt>{field.key}</dt>
                <dd>{field.value}</dd>
              {/each}
              {#if waits.length > 0}
                <dt>Waits for</dt>
                <dd>{waits.map((line) => line.text).join("\n")}</dd>
              {/if}
            </dl>
          {/if}
        </section>
      {/if}
    </div>
  {:else}
    <div class="shown">
      <Sections
        sections={detail.sections}
        {technical}
        {pending}
        ontechnical={() => (technical = !technical)}
        onpending={() => (pending = !pending)}
      />
    </div>
  {/if}
</div>

<style>
  /* A plain text switch, a level below the Detail's chips: the chosen view dark and bold, the
   * other light. Hover only highlights. */
  .views {
    grid-row: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    padding: 0 var(--pad) 8px;
  }
  .view {
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: var(--muted);
    font-weight: 400;
    cursor: pointer;
  }
  .view:hover {
    color: var(--text);
  }
  .view[aria-selected="true"] {
    color: var(--text);
    font-weight: 700;
  }
  .view:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .bar {
    color: var(--faint);
  }
  .summary {
    grid-row: 2;
    margin: 0 0 10px;
    padding: 0 var(--pad);
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  /* The status and REUSED share the status colour; what it cost stays grey. */
  .status {
    font-weight: 700;
    margin-right: 10px;
  }
  .cost {
    color: var(--muted);
    font-weight: 400;
  }
  /* Sizes flow from the Detail down: every column is at most the width it is given. The
   * switch and the summary take their rows only when shown. */
  .full {
    height: 100%;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    outline: none;
    min-height: 0;
  }
  .shown {
    grid-row: 3;
    min-width: 0;
    min-height: 0;
  }
  .card {
    padding: 0 var(--pad) 12px;
    overflow-wrap: anywhere;
  }
  .card p {
    margin: 0 0 4px;
    white-space: pre-wrap;
  }
  .instruction {
    margin-bottom: 14px !important;
  }
  .block {
    margin-bottom: 12px;
  }
  .block h3,
  .fold {
    margin: 0 0 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .fold {
    border: 0;
    background: none;
    padding: 0;
    cursor: pointer;
    font-family: inherit;
  }
  .key,
  .muted {
    color: var(--muted);
  }
  .count {
    color: var(--muted);
  }
  .items {
    margin: 0;
    padding-left: 20px;
  }
  .items li {
    margin-bottom: 8px;
  }
  .head {
    font-weight: 600;
  }
  .error {
    color: var(--red);
  }
  dl {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 16px;
    margin: 4px 0 0;
  }
  dt {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .evidence {
    padding: 0 var(--pad) 12px;
  }
  .evidence h3 {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--accent);
    overflow-wrap: anywhere;
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 10px 12px;
  }
</style>
