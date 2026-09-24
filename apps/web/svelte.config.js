import adapter from '@sveltejs/adapter-static'

/**
 * One source, three flavors:
 * - browser (`build/`, fallback 200.html): served by `spacelens serve` and
 *   deployed to Cloudflare as a PWA.
 * - desktop (`build-desktop/`, fallback index.html): embedded into the Tauri
 *   app, whose asset protocol answers the root document directly.
 * - embed (`build-embed/`, fallback 200.html): built for a `SPACLENS_EMBED_BASE`
 *   mount prefix (e.g. the DSH plugin serving the workbench behind
 *   `/space-lens`); service workers are off and every URL carries the base.
 * A fresh output dir per flavor keeps a test build of one from corrupting the
 * other. Server routes do not exist (`strict: true` fails the build on one).
 */
const desktop = process.env.SPACLENS_BUILD_TARGET === 'desktop'
const embed = process.env.SPACLENS_BUILD_TARGET === 'embed'
const outDir = desktop ? 'build-desktop' : embed ? 'build-embed' : 'build'
const base = embed ? (process.env.SPACLENS_EMBED_BASE ?? '').replace(/\/$/, '') : ''

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    paths: { base },
    adapter: adapter({
      pages: outDir,
      assets: outDir,
      fallback: desktop ? 'index.html' : '200.html',
      precompress: false,
      strict: true,
    }),
    serviceWorker: {
      // Tauri's tauri:// origin forbids service workers; an embed lives under
      // a foreign mount prefix where a worker would only add cache risk.
      register: !desktop && !embed,
    },
  },
}

export default config
