// Start one session and hold it, for the teardown test: prints the session's display,
// process groups and fixture root as one JSON line, then waits to be ended from outside.
import { createGraph, verifyWaiting } from "./fixture.ts";
import { checkBuild, start } from "./driver.ts";

checkBuild();
const fixture = createGraph();
// Teardown owns the root before verify or app setup can fail or block.
console.log(JSON.stringify({ root: fixture.root }));
await verifyWaiting(fixture);
const session = await start(fixture);
console.log(JSON.stringify({ display: session.display, groups: session.groups, root: fixture.root }));
setInterval(() => {}, 1 << 30);
