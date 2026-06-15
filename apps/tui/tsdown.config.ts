import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: ['src/index.ts', 'src/cli.ts', 'src/web-api.ts', 'src/web-cli.ts', 'src/web-service.ts'],
  clean: true,
  dts: true,
  exports: false,
  format: 'esm',
  platform: 'node',
  shims: true,
  sourcemap: false,
})
