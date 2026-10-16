import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  build: { outDir: "dist", emptyOutDir: true },
  // changeOrigin sets Host to the target's, which is what the server's Host check expects.
  server: { proxy: { "/api": { target: "http://127.0.0.1:7777", changeOrigin: true } } },
});
