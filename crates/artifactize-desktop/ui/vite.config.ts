import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

/** The project's brand marks, imported by the app and tests. */
const brand = fileURLToPath(new URL("../../../assets/brand", import.meta.url));
/** The real installable plugin is an explicit input of the bridge integration test only. */
const htmlExample = fileURLToPath(new URL("../../../examples/plugins/html/viewer.html", import.meta.url));

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
    alias: [{ find: /^@html-example(?=\?|$)/, replacement: htmlExample }],
  },
});
