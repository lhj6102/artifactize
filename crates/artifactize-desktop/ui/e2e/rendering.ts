// Pixel observations of actual WebView screenshots. No stylesheet declarations or app state
// stand in for the painted text; screenshot scale comes from the viewport, not an OS guess.
import { PNG } from "pngjs";

/** The strongest text/background separation in the input's painted content area. Compare an
 * empty placeholder with the same glyphs entered normally, through real user input. */
export async function textContrast(browser: WebdriverIO.Browser, selector: string): Promise<number> {
  const frame = await browser.execute((selector) => {
    const field = document.querySelector<HTMLInputElement>(selector)!;
    const box = field.getBoundingClientRect();
    const colour = getComputedStyle(field).backgroundColor.match(/[\d.]+/g)!.slice(0, 3).map(Number);
    return { left: box.left + 8, top: box.top + 5, right: Math.min(box.right - 8, box.left + 100), bottom: box.bottom - 5, width: innerWidth, colour };
  }, selector);
  const image = PNG.sync.read(Buffer.from(await browser.takeScreenshot(), "base64"));
  const scale = image.width / frame.width;
  let strongest = 0;
  for (let y = Math.ceil(frame.top * scale); y < Math.floor(frame.bottom * scale); y += 1) {
    for (let x = Math.ceil(frame.left * scale); x < Math.floor(frame.right * scale); x += 1) {
      const pixel = (y * image.width + x) * 4;
      const separation = Math.hypot(...frame.colour.map((background, channel) => image.data[pixel + channel] - background));
      strongest = Math.max(strongest, separation);
    }
  }
  return strongest;
}

/** A computed text colour is subdued neutral grey, not just a different verdict hue. */
export function subdued(colour: number[], ink: number[], background: number[]): boolean {
  const separation = (text: number[]) => Math.hypot(...text.map((value, channel) => value - background[channel]));
  // Neutral UI greys may carry a slight cool/warm tint, but not a status colour's saturation.
  return Math.max(...colour) - Math.min(...colour) < 48 && separation(colour) < separation(ink) && separation(colour) > 0;
}

export type ViewportBox = { left: number; right: number; top: number; bottom: number };

/** Pixels in viewport coordinates, from one real WebDriver capture. */
export async function screenshotPixels(browser: WebdriverIO.Browser, viewport: { width: number; height: number }) {
  const image = PNG.sync.read(Buffer.from(await browser.takeScreenshot(), "base64"));
  const sx = image.width / viewport.width;
  const sy = image.height / viewport.height;
  function rgb(x: number, y: number): number[] {
    if (x < 0 || y < 0 || x >= image.width || y >= image.height) throw new Error("screenshot sample falls outside the viewport");
    const at = (y * image.width + x) * 4;
    return [...image.data.subarray(at, at + 3)];
  }
  return {
    at: (x: number, y: number) => rgb(Math.round(x * sx), Math.round(y * sy)),
    every: (box: ViewportBox, matches: (color: number[]) => boolean) => {
      const left = Math.ceil(box.left * sx), right = Math.floor(box.right * sx);
      const top = Math.ceil(box.top * sy), bottom = Math.floor(box.bottom * sy);
      if (left >= right || top >= bottom) throw new Error("screenshot sample has no pixels");
      for (let y = top; y < bottom; y += 1) for (let x = left; x < right; x += 1) {
        if (!matches(rgb(x, y))) return false;
      }
      return true;
    },
  };
}
