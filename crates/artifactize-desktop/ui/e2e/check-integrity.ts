// The suite's E2E-INPUT and E2E-PROVIDER-UPSTREAM are checked before lint/typechecking it; the
// native text-input device has its own check (`node lint/e2e-integrity.ts device`).
import { checkSuite } from "../lint/e2e-integrity.ts";
checkSuite();
