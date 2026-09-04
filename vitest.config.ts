import { resolve } from 'path';
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@core': resolve('session-core/pkg')
    }
  },
  test: {
    // A happy-dom instance per file cost more than every test body put together. The
    // files that need one declare it with a `// @vitest-environment happy-dom` docblock.
    environment: 'node',
    setupFiles: ['./vitest.setup.ts']
  }
});
