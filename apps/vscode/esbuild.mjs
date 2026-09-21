import esbuild from 'esbuild'

// Host bundle: everything except `vscode` is inlined; the vsix ships with zero
// runtime node_modules.
await esbuild.build({
  entryPoints: ['src/extension.ts'],
  bundle: true,
  format: 'cjs',
  platform: 'node',
  target: 'node20',
  external: ['vscode'],
  outfile: 'dist/extension.js',
  sourcemap: false,
  logLevel: 'info',
})
