import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: './',
  // skeinmap resolves its optional WebAssembly with `new URL(..., import.meta.url)`;
  // pre-bundling would break that path in dev.
  optimizeDeps: { exclude: ['skeinmap'] },
  build: {
    target: 'es2022',
    cssCodeSplit: false,
    // Everything, including the .wasm, ends up inline in one offline HTML file.
    assetsInlineLimit: 100_000_000,
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
  test: { environment: 'node', include: ['tests/**/*.test.ts'] },
});
