import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// The webview builds one IIFE script + one stylesheet that the extension host
// loads via asWebviewUri (CSS is inlined into the HTML so the CSP stays at
// default-src 'none').
export default defineConfig({
  root: import.meta.dirname,
  plugins: [svelte()],
  build: {
    outDir: resolve(import.meta.dirname, '../dist/webview'),
    emptyOutDir: true,
    lib: {
      entry: resolve(import.meta.dirname, 'main.ts'),
      name: 'SpaceLensWebview',
      formats: ['iife'],
      fileName: () => 'workbench.js',
      cssFileName: 'workbench',
    },
    cssCodeSplit: false,
  },
})
