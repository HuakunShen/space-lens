import adapter from '@sveltejs/adapter-static'

/**
 * SvelteKit serves the standalone browser and Tauri flavors:
 * - browser (`build/`, fallback 200.html): served by `spacelens serve` and
 *   deployed to Cloudflare as a PWA.
 * - desktop (`build-desktop/`, fallback index.html): embedded into the Tauri
 *   app, whose asset protocol answers the root document directly.
 * The two Xross outputs use a separate Vite entry (xross-pack.html), so its
 * native view never bundles SvelteKit's legacy HTTP/Tauri route. A fresh
 * output dir per flavor keeps a test build of one from corrupting the
 * other. Server routes do not exist (`strict: true` fails the build on one).
 */
const desktop = process.env.SPACLENS_BUILD_TARGET === 'desktop'
const outDir = desktop ? 'build-desktop' : 'build'

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter({
      pages: outDir,
      assets: outDir,
      fallback: desktop ? 'index.html' : '200.html',
      precompress: false,
      strict: true,
    }),
    serviceWorker: {
      // Tauri's tauri:// origin forbids service workers
      register: !desktop,
    },
  },
}

export default config
