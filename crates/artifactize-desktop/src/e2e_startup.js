// Observation only, installed before frontend modules run. No application command or
// input handler is exposed; failed startup remains failed with its original error.
(() => {
  const errors = [];
  // Keep the first failures that explain startup, rather than an unbounded error loop.
  const maximumErrors = 32;
  const record = (error) => {
    if (errors.length < maximumErrors) errors.push(Object.freeze(error));
  };
  Object.defineProperty(window, "__ARTIFACTIZE_E2E_ERRORS__", {
    get: () => Object.freeze(errors.slice()), configurable: false,
  });
  window.addEventListener("error", (event) => {
    record({
      kind: "error", message: event.message ?? "Resource failed to load",
      source: event.filename ?? event.target?.src ?? event.target?.href ?? "",
      line: event.lineno ?? 0, column: event.colno ?? 0,
      stack: event.error?.stack ?? "",
    });
  }, true);
  window.addEventListener("unhandledrejection", (event) => {
    record({ kind: "unhandledrejection", message: String(event.reason), stack: event.reason?.stack ?? "" });
  });
  window.addEventListener("securitypolicyviolation", (event) => {
    record({ kind: "csp", directive: event.violatedDirective, blocked: event.blockedURI, source: event.sourceFile, line: event.lineNumber });
  });
})();
