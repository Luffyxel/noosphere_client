import tailwindcss from '@tailwindcss/postcss';
import react from '@vitejs/plugin-react';
import path from 'node:path';
import { cpSync, mkdirSync } from 'node:fs';
import { defineConfig } from 'vite';

let buildDirectory = 'dist-tauri';

export default defineConfig({
  clearScreen: false,
  css: { postcss: { plugins: [tailwindcss()] } },
  envPrefix: ['VITE_'],
  optimizeDeps: { include: ['mammoth', 'cfb', 'docx', 'pdfjs-dist'] },
  plugins: [
    react(),
    {
      name: 'pdf-assets',
      configResolved(config) {
        buildDirectory = config.build.outDir;
      },
      closeBundle() {
        const output = path.resolve(buildDirectory, 'pdf');
        mkdirSync(output, { recursive: true });
        for (const directory of ['cmaps', 'standard_fonts', 'wasm'])
          cpSync(
            path.resolve('node_modules/pdfjs-dist', directory),
            path.join(output, directory),
            { recursive: true },
          );
        cpSync(
          path.resolve('node_modules/pdfjs-dist/LICENSE'),
          path.join(output, 'LICENSE'),
        );
      },
    },
  ],
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
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [{ name: 'ui', test: /node_modules[\\/]@base-ui[\\/]/ }],
        },
      },
    },
  },
  server: {
    host: '127.0.0.1',
  },
});
