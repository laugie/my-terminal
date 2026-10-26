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
  build: {
    // three.js is about 580 kB and sits in its own chunk, loaded only when the
    // 3D topology opens. The app is local, so the size costs no download.
    chunkSizeWarningLimit: 700,
  },
});
