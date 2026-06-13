import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: ['src/index.ts', 'src/cli.ts'],
  clean: true,
  dts: true,
  exports: false,
  format: 'esm',
  platform: 'node',
  shims: true,
  sourcemap: false,
})
