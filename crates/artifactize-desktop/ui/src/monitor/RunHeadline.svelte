<script lang="ts">
  // The shown Run's headline in the Main area's header row: its verdict's icon and word in its
  // colour, then what needs attention, each eval that is RED or ERROR and each sign-off that
  // waits by its status's icon and its name, the failures first, cut at the row's end. A click
  // opens the Run's Detail, which holds its tally, its time and its comment.
  import type { RunPanel } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import { statusIcon } from "../shell/icons";
  import { tone } from "../shell/tone";

  let { run, onrun }: { run: RunPanel; onrun: () => void } = $props();
</script>

<button class="headline" aria-label="{run.mark.status}{run.needs.length > 0 ? `: ${run.needs.map((need) => `${need.text} ${need.mark.status}`).join(", ")}` : ""}, open the Run's Detail" onclick={onrun}>
  <span class="status {tone(run.mark.status)}"><Icon name={statusIcon(run.mark.status)} /> {run.mark.status}</span>
  {#each run.needs as need, at (at)}
    <span class="need"><span class={tone(need.mark.status)}><Icon name={statusIcon(need.mark.status)} /></span>{need.text}</span>
  {/each}
</button>

<style>
  .headline {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    height: var(--control);
    margin: 0;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-small);
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
  }
  .headline:hover {
    background: var(--surface-group);
  }
  .status {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .need {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .need > span {
    display: inline-flex;
  }
</style>
