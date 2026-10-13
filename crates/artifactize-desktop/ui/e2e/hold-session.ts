// Start one session and hold it, for the teardown test: prints the session's display,
// process groups and fixture root as one JSON line, then waits to be ended from outside.
import { create } from "./fixture.ts";
import { start } from "./driver.ts";

const fixture = create();
const session = await start(fixture);
console.log(JSON.stringify({ display: session.display, groups: session.groups, root: fixture.root }));
setInterval(() => {}, 1 << 30);
