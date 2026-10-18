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
