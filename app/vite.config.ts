import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// Tauri loads the dev server on a fixed port, and the built files from dist/.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // WebView2 is Chromium based and updated with Windows.
    target: "chrome120",
    outDir: "dist",
  },
});
