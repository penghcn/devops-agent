import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { env } from 'process'

export default defineConfig({
  plugins: [vue()],
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
