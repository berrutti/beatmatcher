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
    environment: 'happy-dom',
    setupFiles: ['./vitest.setup.ts']
  }
});
