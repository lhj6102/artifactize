// The mechanical part of E2E-INPUT (no injection API, an unmodified provider) is checked before
// the suite is linted and type-checked; the native text-input device has its own check
// (`node lint/e2e-integrity.ts device`).
import { checkSuite } from "../lint/e2e-integrity.ts";
checkSuite();
