import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/dist/**'],
    },
  },
  resolve: {
    deduplicate: ['react', 'react-dom'],
  },
  optimizeDeps: {
    include: [
      '@jelper/component',
      'shallowequal',
      'zustand',
      'react',
      'react-dom',
    ],
  },
});
