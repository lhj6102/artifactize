<script lang="ts">
  // One of the app's own marks (icons.ts) on its 12px box, in the colour of the text around it.
  // With a label it is read aloud as what it means; without one it is decoration beside a word.
  import { ICONS, type IconName } from "./icons";

  let { name, label = null }: { name: IconName; label?: string | null } = $props();
</script>

<svg
  class="icon"
  data-icon={name}
  viewBox="0 0 12 12"
  role={label === null ? undefined : "img"}
  aria-label={label ?? undefined}
  aria-hidden={label === null ? "true" : undefined}
>
  {#each ICONS[name] as shape, at (at)}
    <path
      d={shape.d}
      fill={shape.fill ? "currentColor" : "none"}
      stroke={shape.fill ? "none" : "currentColor"}
      stroke-dasharray={shape.dash?.join(" ")}
    />
  {/each}
</svg>

<style>
  .icon {
    flex: none;
    width: var(--icon);
    height: var(--icon);
    stroke-width: var(--icon-stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
    vertical-align: -1px;
    overflow: visible;
  }
</style>
