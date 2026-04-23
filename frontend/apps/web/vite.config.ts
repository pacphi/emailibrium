import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import pkg from './package.json' with { type: 'json' };

export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  test: {
    exclude: ['e2e/**', 'node_modules/**'],
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 3000,
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
  optimizeDeps: {
    exclude: ['node-llama-cpp'],
  },
  build: {
    sourcemap: true,
    rolldownOptions: {
      external: [
        'node-llama-cpp',
        'ipull',
        'fs',
        'fs/promises',
        'path',
        'os',
        'child_process',
        'worker_threads',
        'crypto',
      ],
      output: {
        manualChunks(id: string) {
          if (id.includes('node_modules/react-dom') || id.includes('node_modules/react/')) {
            return 'vendor';
          }
          if (id.includes('node_modules/@tanstack/react-router')) {
            return 'router';
          }
          if (id.includes('node_modules/@tanstack/react-query')) {
            return 'query';
          }
          if (
            id.includes('node_modules/cmdk') ||
            id.includes('node_modules/framer-motion') ||
            id.includes('node_modules/recharts')
          ) {
            return 'ui';
          }
        },
      },
    },
  },
});
