// The second spec of a scenario whose app is started again: the service has started the app
// again, its first spec having quit it, and the scenario's restarted phase runs on it.
import { setVariable } from "./platform.ts";

setVariable("DESKTOP_TEST_PHASE", "restarted");
await import("./journey.spec.ts");
