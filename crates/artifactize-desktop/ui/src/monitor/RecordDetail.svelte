<script lang="ts">
  // The read-only Detail of a request that is not an open Human review, of an Artifact or of a
  // Run, in sections under header rows that fold by their names (Section). An eval's Summary is
  // its status line (with what running it took and spent, or what reuse saved), its
  // instruction and its Result, then Execution options, Execution metadata and Fingerprint and
  // Debug, folded; on a Current, what verify would do and why in place of the status, and the
  // result it would reuse. A request with a log switches between its Summary and its log: an
  // Agent's live transcript, or a runtime's saved output. An Artifact's Detail is its Path and
  // Debug; a Run's leads with its Outcome and what it Waits for, then What, Provenance and
  // Technical.
  import type { DetailView, Field } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import Section from "../shell/Section.svelte";
  import { markIcon, statusIcon } from "../shell/icons";
  import { tone } from "../shell/tone";
  import Transcript from "./Transcript.svelte";

  let {
    detail,
    log = $bindable(),
  }: {
    detail: DetailView;
    /** The log rather than the Summary; the caller owns it. */
    log: boolean;
  } = $props();

  const agent = $derived(detail.profile === "agent" && detail.request !== null);
  /** The name of the request's log, if it has one. */
  const logName = $derived(agent ? "Agent log" : detail.evidence ? "Runtime log" : null);
  const showLog = $derived(log && logName !== null);
  const card = $derived(detail.card);

  /** The Summary's own facts, each shown in its place rather than under Debug: its options,
   * how it ran (its times and usage), its fingerprint. */
  const OPTIONS = ["Profile", "Options"];
  const METADATA = ["Timing", "Usage", "Saved usage", "Work", "Budget"];
  const SHOWN = new Set(["Status", "Instruction", "Result", "Error", "Fingerprint", ...OPTIONS, ...METADATA]);
  /** An Artifact's evals are listed above its Detail already. */
  const ARTIFACT_SHOWN = new Set(["Status", "Path", "Evals"]);

  const fields = $derived(detail.sections.flatMap((section) => section.fields));
  const waits = $derived(detail.sections.flatMap((section) => section.waits));
  const pick = (keys: string[]): Field[] => fields.filter((field) => keys.includes(field.key));
  const debugFields = $derived(fields.filter((field) => !(detail.target === "artifact" ? ARTIFACT_SHOWN : SHOWN).has(field.key)));

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
  /** Beside the status, in secondary ink: what it took and spent, its model, how it ended. */
  const cost = $derived(
    [...spent, ...(card?.model && detail.profile !== "human" ? [card.model] : []), ...(card?.exitCode != null ? [`exit ${card.exitCode}`] : [])].join(" · "),
  );
  const hasResult = $derived(card !== null && (card.result !== null || card.error !== null));
  const options = $derived(pick(OPTIONS));
  const metadata = $derived(pick(METADATA));
  const fingerprint = $derived(pick(["Fingerprint"]));
  /** A Run's sections after its outcome and what it waits for. */
  const runSections = $derived(detail.sections.filter((section) => section.name !== "outcome" && section.name !== "waits" && section.fields.length > 0));
  /** A Run's other outcome facts (its errors, what waits or runs), each said once. */
  const outcomeFields = $derived(detail.sections.find((section) => section.name === "outcome")?.fields ?? []);
  const words = (status: string) => status.replaceAll("_", " ");
</script>

{#snippet facts(list: Field[] | [string, string][])}
  <dl>
    <!-- A Detail can repeat a key: key by position. -->
    {#each list as fact, at (at)}
      {@const [key, value] = Array.isArray(fact) ? fact : [fact.key, fact.value]}
      <dt>{key}</dt>
      <dd>{value}</dd>
    {/each}
  </dl>
{/snippet}

{#snippet debug()}
  {#if debugFields.length > 0 || waits.length > 0}
    <Section name="Debug">
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
    </Section>
  {/if}
{/snippet}

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
  {#if showLog && agent && detail.request}
    <!-- A log replaces everything below the switch. -->
    <div class="shown">
      {#key detail.request}
        <Transcript id={detail.request} />
      {/key}
    </div>
  {:else if showLog && detail.evidence}
    <div class="shown evidence">
      <pre aria-label={detail.evidence.title}>{detail.evidence.text}</pre>
    </div>
  {:else if detail.target === "run" && detail.outcome}
    {@const outcome = detail.outcome}
    <div class="shown sections">
      <Section name="Outcome">
        <dl>
          {#if detail.mark}
            <dt>Status</dt>
            <dd class="status {tone(detail.mark.status)}"><Icon name={statusIcon(detail.mark.status)} /> {words(detail.mark.status)}</dd>
          {/if}
          {#if outcome.failed.length > 0}
            <dt>Failed</dt>
            <dd class="marks">
              {#each outcome.failed as failed, at (at)}
                <span><span class={tone(failed.mark.status)}><Icon name={statusIcon(failed.mark.status)} label={words(failed.mark.status)} /></span>{failed.text}</span>
              {/each}
            </dd>
          {/if}
          {#if outcome.tally.length > 0}
            <dt>Tally</dt>
            <dd class="marks">
              {#each outcome.tally as counted, at (at)}
                <span><span class={tone(counted.mark.status)}><Icon name={statusIcon(counted.mark.status)} /></span>{counted.count} {words(counted.mark.status)}</span>
              {/each}
            </dd>
          {/if}
          <dt>{outcome.finished ? "Took" : "Running for"}</dt>
          <dd>{outcome.took}</dd>
          {#if outcome.comment}
            <dt>Comment</dt>
            <dd>{outcome.comment}</dd>
          {/if}
          {#each outcomeFields as field, at (at)}
            <dt>{field.key}</dt>
            <dd>{field.value}</dd>
          {/each}
        </dl>
      </Section>
      {#if waits.length > 0}
        <Section name="Waits for">
          <ul class="waits">
            {#each waits as line, at (at)}
              <li class:pending={line.pending}>{line.text}</li>
            {/each}
          </ul>
        </Section>
      {/if}
      {#each runSections as section (section.name)}
        <Section name={section.title}>
          {@render facts(section.fields)}
        </Section>
      {/each}
    </div>
  {:else if detail.target === "artifact"}
    <div class="shown sections">
      {#each pick(["Path"]) as field (field.key)}
        <Section name="Path">
          <p class="line muted">{field.value}</p>
        </Section>
      {/each}
      {@render debug()}
    </div>
  {:else if card}
    <div class="shown">
      {#if detail.plan}
        <!-- A Current: what verify would do, and why, in place of a status. -->
        <p class="summary">
          <span class="status {tone(detail.plan.tone)}">{#if markIcon(detail.plan.glyph, detail.plan.tone)}<Icon name={markIcon(detail.plan.glyph, detail.plan.tone)!} />{/if} {detail.plan.text}</span>
          <span class="cost">{detail.plan.reason}</span>
        </p>
      {:else if detail.mark}
        <p class="summary">
          <span class="status {tone(detail.mark.status)}"><Icon name={statusIcon(detail.mark.status)} /> {words(detail.mark.status)}{card.reused ? " · REUSED" : ""}</span>
          {#if cost}<span class="cost">{cost}</span>{/if}
        </p>
      {/if}
      {#if card.instruction}<p class="instruction">{card.instruction}</p>{/if}
      <div class="sections">
        {#if hasResult}
          <Section name="Result">
            {#if card.error}<p class="error">{card.error}</p>{/if}
            {#each card.result ?? [] as entry (entry.key)}
              {#if entry.kind === "text"}
                <p class="text">{#if entry.key !== "summary"}<span class="key">{entry.key}</span> {/if}{entry.text}</p>
              {:else}
                <p class="key">{entry.key}</p>
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
          </Section>
        {/if}
        {#if options.length > 0}
          <Section name="Execution options">{@render facts(options)}</Section>
        {/if}
        <!-- How a past run ran and its fingerprint belong to that run, not to a Current; what the
             status line says (its time, tokens and exit) is not said again. -->
        {#if !detail.plan && metadata.length > 0}
          <Section name="Execution metadata">{@render facts(metadata)}</Section>
        {/if}
        {#if !detail.plan && fingerprint.length > 0}
          <Section name="Fingerprint">{@render facts(fingerprint)}</Section>
        {/if}
        {@render debug()}
      </div>
    </div>
  {:else}
    <div class="shown">
      {#if detail.summary}<p class="summary {tone(detail.mark?.status)}">{detail.summary}</p>{/if}
      <div class="sections">
        {#each detail.sections as section (section.name)}
          {#if section.fields.length > 0}
            <Section name={section.title}>{@render facts(section.fields)}</Section>
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  /* A plain text switch, a level below the Detail's chips: the chosen view dark and bold, the
   * other light. Hover only highlights. */
  .views {
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
    color: var(--ink-2);
    font-weight: 400;
    cursor: pointer;
  }
  .view:hover {
    color: var(--ink);
  }
  .view[aria-selected="true"] {
    color: var(--ink);
    font-weight: 700;
  }
  .view:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .bar {
    color: var(--ink-3);
  }
  /* Sizes flow from the Detail down: every column is at most the width it is given. */
  .full {
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .full:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .shown {
    grid-row: 2;
    overflow: auto;
    min-width: 0;
    min-height: 0;
    overflow-wrap: anywhere;
  }
  /* The status line: the verdict's icon and word bold in its colour, the rest in secondary ink. */
  .summary {
    margin: 0 0 8px;
    padding: 0 var(--pad);
    overflow-wrap: anywhere;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 700;
    margin-right: 10px;
  }
  dd.status {
    display: flex;
    margin-right: 0;
  }
  .cost {
    color: var(--ink-2);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  .instruction {
    margin: 0;
    padding: 0 var(--pad) 10px;
    white-space: pre-wrap;
  }
  .sections p {
    margin: 0 0 4px;
    white-space: pre-wrap;
  }
  .key,
  .muted {
    color: var(--ink-2);
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
  /* Header → label → value: keys right-aligned in secondary ink, values in tabular figures. */
  dl {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 16px;
    margin: 0;
  }
  dt {
    color: var(--ink-2);
    text-align: right;
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  .marks {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    white-space: normal;
  }
  .marks > span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .marks > span > span {
    display: inline-flex;
  }
  .waits {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .pending {
    padding-left: 18px;
    color: var(--ink-2);
  }
  .evidence {
    padding: 0 var(--pad) 12px;
  }
  /* Saved output: one step below the panel, outlined by the faintest line. */
  pre {
    margin: 0;
    font: var(--text-small) / 1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-input);
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    padding: 6px 8px;
  }
</style>
