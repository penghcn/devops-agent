import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { env } from 'process'

export default defineConfig({
  plugins: [tailwindcss(), vue()],
  build: {
    cssMinify: 'esbuild',
  },
  server: {
    port: parseInt(env.FRONTEND_PORT || '', 10) || 3000,
    proxy: {
      '/api': {
        target: `http://localhost:${env.BACKEND_PORT || 8080}`,
        changeOrigin: true,
      },
    },
  },
})
