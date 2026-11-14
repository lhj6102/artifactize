<script lang="ts">
  // The Navigator: the whole graph in small, each Artifact a block of its status's colour, the
  // part in view framed. It shows only while part of the graph lies outside the view. A click or
  // a drag in it asks for that place in the middle of the view, the zoom kept; hovering it opens
  // nothing.
  import type { Viewport } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import type { Light } from "../ipc/client";
  import { LIGHTS } from "../shell/icons";
  import type { Point } from "./graph";
  import type { Scene } from "./scene";

  let {
    laid,
    lights,
    viewport,
    width,
    height,
    onmove,
  }: {
    /** Where every copy is. */
    laid: Scene;
    /** Each Artifact's light by its tree id. */
    lights: Map<string, Light>;
    viewport: Viewport;
    /** The canvas's size in pixels. */
    width: number;
    height: number;
    /** Put this point of the graph in the middle of the view. */
    onmove: (point: Point) => void;
  } = $props();

  /** The Navigator's width in pixels, whatever the graph's shape: a tall graph's drawing sits in
   * its middle. Its height follows the graph's proportions. */
  const WIDE = 166;
  /** The most it is tall, so a tall graph does not cover the canvas. */
  const HIGH = 120;
  /** The least it is tall, so a flat graph (a long chain) stays a box to point at; the drawing
   * sits in its middle. */
  const LOW = 24;

  const bounds = $derived(laid.bounds);
  /** Graph pixels per Navigator pixel, the whole graph fitting its box. */
  const scale = $derived(Math.max(bounds.width / WIDE, bounds.height / HIGH, 1e-6));
  const size = $derived({ width: Math.max(1, Math.round(bounds.width / scale)), height: Math.max(1, Math.round(bounds.height / scale)) });
  /** The box's height, and how far right and down in it the drawing starts. */
  const box = $derived(Math.max(size.height, LOW));
  const inset = $derived((WIDE - size.width) / 2);
  const offset = $derived((box - size.height) / 2);
  /** The part in view, in the box's pixels, clipped to the box. */
  const frame = $derived.by(() => {
    const left = (-viewport.x / viewport.zoom - bounds.x) / scale + inset;
    const top = (-viewport.y / viewport.zoom - bounds.y) / scale + offset;
    const [right, bottom] = [left + width / viewport.zoom / scale, top + height / viewport.zoom / scale];
    const [x, y] = [Math.max(0, left), Math.max(0, top)];
    return { x, y, width: Math.max(0, Math.min(WIDE, right) - x), height: Math.max(0, Math.min(box, bottom) - y) };
  });

  /** Paint each Artifact that is no box as a block of its light's colour. */
  const paint: Attachment<HTMLCanvasElement> = (canvas) => {
    const ratio = window.devicePixelRatio || 1;
    [canvas.width, canvas.height] = [Math.round(size.width * ratio), Math.round(size.height * ratio)];
    const context = canvas.getContext("2d");
    if (!context) return;
    const style = getComputedStyle(document.documentElement);
    const paths: Partial<Record<Light, Path2D>> = {};
    laid.places.forEach((place, number) => {
      if (laid.wholes.has(number)) return;
      const light = lights.get(laid.nodes[number]) ?? "waiting";
      const path = (paths[light] ??= new Path2D());
      path.rect((place.x - bounds.x) / scale, (place.y - bounds.y) / scale, Math.max(place.width / scale, 1), Math.max(place.height / scale, 1));
    });
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, size.width, size.height);
    for (const [light, path] of Object.entries(paths) as [Light, Path2D][]) {
      context.fillStyle = style.getPropertyValue(LIGHTS[light].colour).trim();
      context.fill(path);
    }
  };

  /** The graph point under the pointer. */
  function at(event: PointerEvent, box: HTMLElement): Point {
    const rect = box.getBoundingClientRect();
    const x = Math.min(Math.max(event.clientX - rect.left - inset, 0), size.width);
    const y = Math.min(Math.max(event.clientY - rect.top - offset, 0), size.height);
    return { x: bounds.x + x * scale, y: bounds.y + y * scale };
  }

  function press(event: PointerEvent) {
    if (event.button !== 0) return;
    const box = event.currentTarget as HTMLElement;
    event.preventDefault();
    box.setPointerCapture(event.pointerId);
    onmove(at(event, box));
    const drag = (moved: PointerEvent) => onmove(at(moved, box));
    const drop = () => {
      box.removeEventListener("pointermove", drag);
      box.removeEventListener("pointerup", drop);
      box.removeEventListener("pointercancel", drop);
    };
    box.addEventListener("pointermove", drag);
    box.addEventListener("pointerup", drop);
    box.addEventListener("pointercancel", drop);
  }
</script>

<div class="navigator nodrag nopan nowheel" data-navigator>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="map" style:width="{WIDE}px" style:height="{box}px" onpointerdown={press}>
    <!-- The part in view under the drawing: the blocks keep their own colours inside it. -->
    <span class="frame" style:left="{frame.x}px" style:top="{frame.y}px" style:width="{frame.width}px" style:height="{frame.height}px"></span>
    <canvas style:left="{inset}px" style:top="{offset}px" style:width="{size.width}px" style:height="{size.height}px" {@attach paint}></canvas>
  </div>
</div>

<style>
  /* It floats as the find field does: raised one step, a panel line, at most the faint shadow. */
  .navigator {
    position: absolute;
    right: calc(var(--detail-cover, 0px) + 8px);
    bottom: 8px;
    z-index: 6;
    padding: 3px;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .map {
    position: relative;
    cursor: pointer;
    touch-action: none;
  }
  canvas {
    position: absolute;
    display: block;
  }
  .frame {
    position: absolute;
    box-sizing: border-box;
    background: color-mix(in srgb, var(--selection) 60%, transparent);
    border: 1px solid var(--outline-selected);
    pointer-events: none;
  }
</style>
