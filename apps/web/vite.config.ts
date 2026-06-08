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
  build: {
    rollupOptions: {
      onwarn(warning, warn) {
        if (warning.code === 'INVALID_ANNOTATION' && warning.id?.includes('@noble/curves')) {
          return
        }
        warn(warning)
      },
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return
          if (id.includes('@mysten') || id.includes('@noble') || id.includes('@scure')) {
            return 'sui-vendor'
          }
          if (id.includes('react') || id.includes('@tanstack')) {
            return 'react-vendor'
          }
          if (id.includes('lucide-react') || id.includes('lottie-react')) {
            return 'ui-vendor'
          }
          return 'vendor'
        },
      },
    },
  },
})
