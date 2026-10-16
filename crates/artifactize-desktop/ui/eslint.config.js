// The frontend checklist's mechanical rules (DEV-FRONTEND.md), run by `pnpm lint` with
// --no-inline-config: no comment can switch a rule off. Generated IPC bindings are excluded.
import svelte from "eslint-plugin-svelte";
import tseslint from "typescript-eslint";
import e2ePlatform from "./lint/e2e-platform.js";
import effectComment from "./lint/effect-comment.js";
import typescript from "./lint/typescript.js";

export default [
  { ignores: ["src/ipc/bindings.ts"] },
  tseslint.configs.base,
  ...svelte.configs["flat/recommended"],
  // The e2e Artifact runs its own lint command over the same configuration; TypeScript's
  // recommended rules cover it and the build configuration.
  ...typescript(["e2e/**/*.ts", "vite.config.ts"]),
  ...e2ePlatform(["e2e/**/*.ts", "e2e/**/*.mjs", "e2e/**/*.js"], "e2e/platform.ts"),
  {
    files: ["src/**/*.svelte", "src/**/*.svelte.ts"],
    languageOptions: {
      parserOptions: { parser: tseslint.parser, extraFileExtensions: [".svelte"] },
    },
  },
  {
    files: ["src/**/*.ts", "src/**/*.svelte"],
    plugins: { local: { rules: { "effect-comment": effectComment } } },
    rules: {
      "svelte/prefer-writable-derived": "error",
      "svelte/no-unnecessary-state-wrap": "error",
      "svelte/require-each-key": "error",
      "local/effect-comment": "error",
    },
  },
];
