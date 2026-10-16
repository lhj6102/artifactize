// The desktop Artifact owns its automation harness. Reuse the frontend's declared lint
// dependencies without checking another Artifact's code.
import typescript from "./ui/lint/typescript.js";

export default typescript(["harness/**/*.mts"]);
