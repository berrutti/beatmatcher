import { resolve } from 'path';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@core': resolve('session-core/pkg')
    }
  },
  test: {
    setupFiles: ['./vitest.setup.ts']
  }
});
