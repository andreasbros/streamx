import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 9000,
    host: "0.0.0.0",
    allowedHosts: ["streamx.cbdemo.net"],
    headers: {
      "Cache-Control": "no-store, no-cache, must-revalidate, proxy-revalidate",
      "Pragma": "no-cache",
      "Expires": "0",
      "Surrogate-Control": "no-store",
    },
    proxy: {
      "/api": {
        target: "http://localhost:8999",
        changeOrigin: true,
        ws: true,
      },
      "/proxy": {
        target: "http://localhost:8999",
        changeOrigin: true,
      },
    },
  },
  preview: {
    port: 9000,
    host: "0.0.0.0",
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
