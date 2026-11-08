import { defineConfig } from "vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// Zwei Seiten: der Rand je Bildschirm und das Einstellungsfenster
export default defineConfig(() => ({
  clearScreen: false,
  build: {
    rollupOptions: {
      input: { overlay: "overlay.html", einstellungen: "einstellungen.html" },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
