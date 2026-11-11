import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// `bun run dev` proxies the game server so the viewer works from the dev server.
export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8090",
      "/stream": { target: "ws://127.0.0.1:8090", ws: true },
    },
  },
});
