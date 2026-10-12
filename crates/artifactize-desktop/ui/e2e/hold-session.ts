// Start one session and hold it, for the teardown test: prints the session's display and
// process groups as one JSON line, then waits to be ended from outside.
import { create } from "./fixture.ts";
import { start } from "./driver.ts";

const session = await start(create());
console.log(JSON.stringify({ display: session.display, groups: session.groups }));
setInterval(() => {}, 1 << 30);
