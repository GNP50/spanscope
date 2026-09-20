import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: './',
  build: { target: 'es2022', cssCodeSplit: false, assetsInlineLimit: 100_000_000 },
  test: { environment: 'node', include: ['tests/**/*.test.ts'] },
});
