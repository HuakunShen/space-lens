import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  define: {
    __SPACLENS_DESKTOP__: JSON.stringify(process.env.SPACLENS_BUILD_TARGET === 'desktop'),
  },
  build: {
    assetsDir: '_app/immutable',
  },
})
