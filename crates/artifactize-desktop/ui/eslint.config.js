// The frontend checklist's mechanical rules (DEV-FRONTEND.md), run by `pnpm lint` with
// --no-inline-config: no comment can switch a rule off. Generated IPC bindings are excluded.
import svelte from "eslint-plugin-svelte";
import tseslint from "typescript-eslint";
import effectComment from "./lint/effect-comment.js";

export default [
  { ignores: ["src/ipc/bindings.ts"] },
  tseslint.configs.base,
  ...svelte.configs["flat/recommended"],
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
