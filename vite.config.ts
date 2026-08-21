import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri supplies this when running `tauri dev --host`, for testing on a
// phone or another machine on the network.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],

  // Do not let Vite clear the screen — it hides Rust compiler errors.
  clearScreen: false,

  server: {
    // Tauri looks for the dev server on a fixed port, so failing loudly is
    // better than silently serving somewhere it will not look.
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },

  build: {
    // Matches the oldest webview each platform ships: Edge WebView2 on
    // Windows, WKWebView on macOS 10.15, WebKitGTK on Linux.
    target: ["es2021", "chrome100", "safari13"],
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
  },
});
