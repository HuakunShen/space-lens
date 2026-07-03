/**
 * Bundles the private Kunkun backend process as a self-contained JS file.
 * All TypeScript/JS workspace dependencies are inlined; the only package kept
 * external is the native `space-lens` scanner package, which is copied into the
 * plugin dist folder with its `.node` artifacts.
 */
export default {
  entry: ["src/backend.ts"],
  clean: false,
  dts: false,
  exports: false,
  format: "esm",
  platform: "node",
  target: "es2022",
  shims: false,
  sourcemap: false,
  outDir: "dist",
  outExtensions: () => ({ js: ".js" }),
  deps: {
    alwaysBundle: [
      /^@kunkunsh\/sdk(?:\/.*)?$/,
      /^@kunkunsh\/observability(?:\/.*)?$/,
      /^@space-lens\/cli(?:\/.*)?$/,
      /^kkrpc(?:\/.*)?$/,
    ],
    neverBundle: ["space-lens"],
    onlyBundle: false,
  },
};
