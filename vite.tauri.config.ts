import { resolve } from 'path';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  root: 'src/renderer',
  plugins: [vue()],
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@core': resolve('session-core/pkg')
    }
  },
  server: {
    port: 1420,
    strictPort: true
  },
  build: {
    outDir: resolve('dist-tauri'),
    emptyOutDir: true
  }
});
