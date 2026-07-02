import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

// @tauri-apps/cli sets TAURI_DEV_HOST for mobile/remote dev.
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // Tauri expects a fixed port and fails if unavailable.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: {
      // Don't watch the Rust backend from the Vite side.
      ignored: ['**/src-tauri/**'],
    },
  },
  // Produce sourcemaps only in debug builds for smaller release bundles.
  build: {
    target: 'es2022',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
