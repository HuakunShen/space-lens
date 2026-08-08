import { resolve } from 'node:path'
import { univiewSolid } from '@uniview/tui-solid/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [univiewSolid()],
  ssr: {
    external: ['space-lens', 'web-tree-sitter'],
    noExternal: ['@uniview/tui-solid', 'solid-js'],
  },
  test: {
    server: {
      deps: {
        inline: ['@uniview/tui-solid', 'solid-js'],
      },
    },
  },
  build: {
    ssr: true,
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        cli: resolve(import.meta.dirname, 'src/cli.ts'),
        index: resolve(import.meta.dirname, 'src/index.ts'),
      },
      output: {
        entryFileNames: '[name].mjs',
        banner: '#!/usr/bin/env node',
      },
    },
  },
})
