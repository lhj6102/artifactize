// Node's unfiltered spec reporter is persisted alongside this view. Native process
// stdout/stderr (including all DBus/portal diagnostics) stays in that complete log;
// the bounded command transcript keeps every test outcome, failure and summary.
import { Readable } from "node:stream";
import { spec, type TestEvent } from "node:test/reporters";

export default async function* concise(source: AsyncIterable<TestEvent>) {
  const rendered = Readable.from(source).compose<Readable>(async function* (events: AsyncIterable<TestEvent>) {
    for await (const event of events) {
      if (event.type !== "test:stdout" && event.type !== "test:stderr") yield event;
    }
  }).compose(spec());
  for await (const chunk of rendered) yield chunk;
}
