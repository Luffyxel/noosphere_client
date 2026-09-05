import tailwindcss from '@tailwindcss/postcss';
import react from '@vitejs/plugin-react';
import path from 'node:path';
import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  css: { postcss: { plugins: [tailwindcss()] } },
  envPrefix: ['VITE_'],
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname),
    },
  },
  build: {
    outDir: 'dist-tauri',
    emptyOutDir: true,
    sourcemap: false,
    target: ['es2021', 'chrome105', 'safari13'],
  },
  server: {
    host: '127.0.0.1',
  },
});
