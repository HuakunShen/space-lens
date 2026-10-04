import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

const devApi = process.env.SPACLENS_DEV_API

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  define: {
    __SPACLENS_DESKTOP__: JSON.stringify(process.env.SPACLENS_BUILD_TARGET === 'desktop'),
  },
  server: devApi
    ? {
        proxy: {
          '/health': { target: devApi, changeOrigin: true },
          '/api': { target: devApi, changeOrigin: true },
        },
      }
    : undefined,
  build: {
    assetsDir: '_app/immutable',
  },
})
