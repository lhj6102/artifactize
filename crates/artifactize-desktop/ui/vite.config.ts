import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

/** The one input the UI takes from outside its folder: the project's brand marks, which the
 * app and its tests import as `@brand/...`. */
const brand = fileURLToPath(new URL("../../../assets/brand", import.meta.url));

// The Tauri window loads the built files (or this dev server on port 1420).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "safari16", outDir: "dist", emptyOutDir: true },
  resolve: { alias: { "@brand": brand }, ...(process.env.VITEST ? { conditions: ["browser"] } : {}) },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
