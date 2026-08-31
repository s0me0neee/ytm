import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// @ts-expect-error process is a nodejs global
const profiling = Boolean(process.env.YTM_PROFILE);

/**
 * Injects React DevTools' standalone backend, and only when `pnpm profile`
 * asked for it.
 *
 * The browser extension cannot load in a WKWebView, so the standalone build
 * (the one React Native uses) is the only way to get a React profiler into
 * the app at all. It has to be hooked up *before* React loads, which is why
 * this goes in ahead of the module script rather than being imported.
 *
 * Behind an env check because the tag names a `localhost` origin that will
 * not exist on anyone else's machine: shipped, it would be a dead request on
 * every launch, and `src-tauri/tauri.dev.conf.json` widens the CSP to let it
 * through only for this same command.
 */
const reactDevtools = {
  name: "react-devtools",
  transformIndexHtml(html: string) {
    return profiling
      ? html.replace("<head>", '<head>\n    <script src="http://localhost:8097"></script>')
      : html;
  },
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), reactDevtools],

  // Compiled away when false, so the `Profiler` wrapper and its interval are
  // not in the shipped bundle at all -- not merely unreachable in it.
  define: { __YTM_PROFILE__: JSON.stringify(profiling) },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
