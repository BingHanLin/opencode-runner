import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite is invoked by `tauri dev` / `tauri build`. The dev server is fixed at
// port 1420 to match `tauri.conf.json#build.devUrl`. `clearScreen: false` and
// `strictPort: true` keep the Tauri terminal output readable.
export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    hmr: { protocol: "ws", host: "localhost", port: 1421 },
    watch: {
      ignored: ["**/src-tauri/**", "**/target/**", "**/data/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
}));
