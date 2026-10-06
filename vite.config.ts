import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port for dev; strictPort so it fails loudly instead
// of silently picking a different one that tauri.conf.json's devUrl
// wouldn't match.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Don't let Vite's watcher react to Rust rebuilds touching src-tauri.
      ignored: ["**/src-tauri/**"],
    },
  },
});
