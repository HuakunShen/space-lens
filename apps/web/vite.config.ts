import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, searchForWorkspaceRoot } from 'vite'

const devApi = process.env.SPACLENS_DEV_API

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  define: {
    __SPACLENS_DESKTOP__: JSON.stringify(process.env.SPACLENS_BUILD_TARGET === 'desktop'),
  },
  // The component library is a workspace source package, not a pre-built
  // dependency. Without these two entries Vite pre-bundles
  // `@space-lens/web-ui` into node_modules/.vite and then refuses to serve the
  // real files (they live outside the app root), so editing a component had no
  // effect until the dev server was restarted.
  optimizeDeps: {
    exclude: ['@space-lens/web-ui'],
  },
  server: {
    fs: {
      allow: [searchForWorkspaceRoot(process.cwd())],
    },
    ...(devApi
      ? {
          proxy: {
            '/health': { target: devApi, changeOrigin: true },
            '/api': { target: devApi, changeOrigin: true },
          },
        }
      : {}),
  },
  build: {
    assetsDir: '_app/immutable',
  },
})
