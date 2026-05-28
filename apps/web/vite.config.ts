/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    port: 43173,
    proxy: {
      '/api': {
        target: 'http://localhost:43001',
        changeOrigin: true,
      },
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    // E2E specs live in e2e/ and are run by Playwright, not vitest.
    exclude: ['**/node_modules/**', '**/dist/**', 'e2e/**'],
  },
})
