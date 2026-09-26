import { sveltekit } from '@sveltejs/kit/vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'
import { fileURLToPath } from 'node:url'

const target = process.env.SPACLENS_BUILD_TARGET
const xross = target === 'xross-local' || target === 'xross-hosted'

export default defineConfig({
  plugins: [tailwindcss(), xross ? svelte() : sveltekit()],
  ...(xross ? { base: './' } : {}),
  define: {
    __SPACLENS_DESKTOP__: JSON.stringify(process.env.SPACLENS_BUILD_TARGET === 'desktop'),
    __SPACLENS_XROSS__: JSON.stringify(process.env.SPACLENS_BUILD_TARGET === 'xross-local' || process.env.SPACLENS_BUILD_TARGET === 'xross-hosted'),
  },
  build: {
    ...(xross ? {
      outDir: `build-${target}`,
      rollupOptions: { input: fileURLToPath(new URL('./xross-pack.html', import.meta.url)) },
    } : {}),
    assetsDir: '_app/immutable',
  },
})
