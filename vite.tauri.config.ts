import { resolve } from 'path';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  root: 'src/renderer',
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src')
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
