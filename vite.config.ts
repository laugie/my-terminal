import { defineConfig } from "vite";

// Tauri dev server: fixed port, and do not watch the Rust project.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
