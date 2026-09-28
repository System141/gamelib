import { connect } from "node:net";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Set by `tauri dev` when running on a device over the network (mobile); unused on desktop.
const host = process.env.TAURI_DEV_HOST;

/** Port of `gamelib-cli serve` (crates/gamelib-cli/src/serve.rs), which the browser preview uses. */
const API_PORT = 1430;

/** Whether `gamelib-cli serve` is listening. */
function apiIsUp(): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = connect({ host: "127.0.0.1", port: API_PORT });
    const done = (up: boolean) => {
      socket.destroy();
      resolve(up);
    };
    socket.setTimeout(500);
    socket.once("connect", () => done(true));
    socket.once("error", () => done(false));
    socket.once("timeout", () => done(false));
  });
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Keep Rust compiler output visible when running under `tauri dev`.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
    proxy: {
      // The browser preview talks to the real catalog through `gamelib-cli serve` (src/mocks/server.ts).
      // 127.0.0.1 rather than localhost: Node may resolve localhost to ::1 only.
      "/api": {
        target: `http://127.0.0.1:${API_PORT}`,
        // Without the server, the health check answers `{ ok: false }` (the UI then uses fixture
        // data) and the event stream 404, instead of proxy errors in the terminal and console.
        bypass: async (req, res) => {
          const url = req.url ?? "";
          if (!/^\/api\/(health|events)\b/.test(url) || (await apiIsUp())) return undefined;
          if (!url.startsWith("/api/health") || !res) return false;
          res.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store" });
          res.end(JSON.stringify({ ok: false }));
          return url;
        },
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
});
