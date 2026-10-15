// The manifest is an offer, not ownership: only an explicit receiving-owner ACK transfers it.
import { createInterface } from "node:readline";
import { type Readable, type Writable } from "node:stream";

/** The receiver registers cleanup responsibility before ACK and waits for the sender's
 * successful exit before using the input. EOF, failed delivery and cancellation reject it. */
export async function acknowledge(manifest: string, input: Readable, output: Writable, signal: AbortSignal): Promise<void> {
  await new Promise<void>((done, fail) => {
    const lines = createInterface({ input });
    let accepted = false;
    let delivered = false;
    let settled = false;
    function finish(error?: unknown) {
      if (settled) return;
      settled = true;
      lines.off("close", ended);
      lines.close();
      input.pause();
      input.off("error", failed);
      output.off("error", failed);
      output.off("close", closed);
      signal.removeEventListener("abort", aborted);
      if (error !== undefined) {
        // Cancel our pending publication too; a failed handoff must not later deliver an offer.
        output.destroy();
        fail(error);
      } else done();
    }
    const failed = (error: unknown) => finish(error);
    const ended = () => { if (!accepted) finish(new Error("candidate receiver closed without accepting cleanup responsibility")); };
    const closed = () => finish(new Error("candidate manifest delivery closed"));
    const aborted = () => finish(signal.reason);
    lines.once("line", (line) => {
      if (line !== `accept ${manifest}`) {
        finish(new Error("candidate receiver did not acknowledge this manifest"));
        return;
      }
      accepted = true;
      if (delivered) finish();
    });
    lines.once("close", ended);
    lines.once("error", failed);
    input.once("error", failed);
    output.once("error", failed);
    output.once("close", closed);
    signal.addEventListener("abort", aborted, { once: true });
    if (signal.aborted) { aborted(); return; }
    if (input.readableEnded || input.destroyed) { ended(); return; }
    try {
      output.write(`${manifest}\n`, (error) => {
        // Writable emits the matching error after invoking this callback; its error
        // listener owns rejection so that the stream failure cannot become unhandled.
        if (error) return;
        delivered = true;
        if (accepted) finish();
      });
    } catch (error) { finish(error); }
  });
}
