import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const stubDir = fileURLToPath(new URL('./tests/__stubs__', import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@tauri-apps/plugin-process': stubDir + '/tauri-apps/plugin-process/index.ts',
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.{ts,tsx}'],
  },
});
