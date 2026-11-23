<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import { clampWidth, KEEP, MIN, saveWidth } from "./detail-width";

  let { children, detail }: { children: Snippet; detail?: Snippet } = $props();
  const app = getAppState();

  /** Dragging the Detail's left edge: the mouse moves it wherever it goes until the button is
   * released, as the pane dividers follow it. */
  function grab(event: MouseEvent) {
    if (event.button !== 0) return;
    const stage = (event.currentTarget as HTMLElement).closest(".stage");
    if (!stage) return;
    event.preventDefault();
    const drag = (moved: MouseEvent) => {
      const room = stage.getBoundingClientRect();
      app.detailWidth = clampWidth(room.right - moved.clientX, room.width);
    };
    const drop = () => {
      window.removeEventListener("mousemove", drag);
      window.removeEventListener("mouseup", drop);
      if (app.detailWidth !== null) saveWidth(app.detailWidth);
    };
    window.addEventListener("mousemove", drag);
    window.addEventListener("mouseup", drop);
  }

  /** Dock at the canvas's top: the stage header's bottom, the graph's tools being in that header
   * (J-DETAIL). The stage's bottom is the canvas's bottom; Chat's composer floats over it. */
  const graphTop: Attachment<HTMLElement> = (stage) => {
    let watched: Element | null = null;
    const read = () => {
      if (!watched) {
        stage.style.removeProperty("--graph-top");
        return;
      }
      const top = watched.getBoundingClientRect().top - stage.getBoundingClientRect().top;
      stage.style.setProperty("--graph-top", `${top}px`);
    };
    const sized = new ResizeObserver(read);
    const find = () => {
      const area = stage.querySelector("[data-graph-area]");
      if (area === watched) return;
      if (watched) sized.unobserve(watched);
      watched = area;
      if (area) sized.observe(area);
      // A Detail can already be open when the lazy graph first mounts. Dock against
      // that layout now; the resize observer follows any later changes.
      read();
    };
    sized.observe(stage);
    const changed = new MutationObserver(find);
    changed.observe(stage, { childList: true, subtree: true });
    find();
    return () => {
      sized.disconnect();
      changed.disconnect();
    };
  };

</script>

<div
  class="stage"
  style:--detail-min="{MIN}px"
  style:--graph-keep="{KEEP}px"
  style:--detail-width={app.detailWidth === null ? null : `${app.detailWidth}px`}
  style:--detail-cover={detail ? "var(--dock-width)" : null}
  {@attach graphTop}
>
  {@render children()}
  {#if detail}
    <aside class="dock" aria-label="Detail">
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="edge" role="separator" aria-orientation="vertical" aria-label="Detail width" onmousedown={grab}></div>
      {@render detail()}
    </aside>
  {/if}
</div>

<style>
  /* The graph's own layers (canvas, Detail) stack inside the stage, so none of them covers the
   * divider's grab zone on the graph's side, and the Detail never sits beside the Pane. */
  /* The Detail's width, as the dock takes it: a drag's, or its default, within its bounds. */
  .stage {
    --dock-width: min(clamp(var(--detail-min), var(--detail-width, min(46%, 680px)), 100%), max(0px, calc(100% - var(--graph-keep))));
    position: relative;
    isolation: isolate;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  /* The inspector: a panel docked at the graph's right edge, panel meeting panel at a line,
   * with the graph visible on its left. It sets its own width, by default or by a drag, within
   * the graph's area (as detail-width.ts fits a drag); its content never sizes it, and wraps or
   * scrolls in its own boxes. */
  /* From the graph's top (--graph-top, the stage header's bottom) to the area's bottom; under
   * the header row alone until a graph is drawn. No accent line marks focus in it (D-MAIN-1). */
  .dock {
    position: absolute;
    z-index: 20;
    top: var(--graph-top, 32px);
    right: 0;
    bottom: 0;
    width: var(--dock-width);
    contain: inline-size;
    overflow: hidden;
    border-top: 1px solid var(--line-group);
    border-left: 1px solid var(--line-panel);
  }
  .edge {
    position: absolute;
    z-index: 1;
    top: 0;
    bottom: 0;
    left: 0;
    width: 6px;
    cursor: col-resize;
    touch-action: none;
  }
</style>
