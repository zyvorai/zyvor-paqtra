// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

const DEV_API = process.env.PAQTRA_DEV_API || 'http://localhost:9191';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/__tests__/setup.ts',
  },
  server: {
    port: 3000,
    proxy: {
      '/api': {
        target: DEV_API,
        changeOrigin: true,
        secure: false,
      },
      '/ws': {
        target: DEV_API.replace(/^http/, 'ws'),
        ws: true,
        secure: false,
      },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    rollupOptions: {
      output: {
        // Vite 8 / Rolldown requires a function (object form is rejected).
        manualChunks(id) {
          if (!id.includes('node_modules')) return;
          if (id.includes('react-dom') || id.includes('react-router') || /[/\\]react[/\\]/.test(id)) {
            return 'react-vendor';
          }
          if (id.includes('@tanstack/react-query')) return 'query-vendor';
          if (id.includes('recharts')) return 'chart-vendor';
          if (id.includes('lucide-react')) return 'icon-vendor';
        },
      },
    },
  },
});
