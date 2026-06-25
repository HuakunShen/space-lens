# 2026-06-25 — Kunkun API optional peer boundary

Closed the Kunkun dependency boundary gap for the shared Space Lens web app.

## What changed

- `apps/web` now declares `@kunkunsh/api` as an optional peer dependency
  instead of relying on an undeclared package import.
- The default Kunkun adapter still lazy-loads the host package at runtime, but
  TypeScript no longer needs the package installed for standalone web checks.
- `apps/kunkun-plugin/scripts/build-backend.ts` documents that the plugin build
  links the local Kunkun checkout to satisfy that optional peer for Kunkun mode.
- `pnpm-lock.yaml` disables auto-installing peers so standalone installs do not
  try to fetch the unpublished Kunkun API package.
- Boundary tests assert that the shared web package keeps `@kunkunsh/api` out of
  normal dependencies.

## Verification

- `bun test apps/web/tests/kunkun-boundary.test.ts apps/web/tests/kunkun-client.test.ts apps/kunkun-plugin/tests/packaging.test.ts`
- `bun test apps/kunkun-plugin/tests/headless-custom-view.test.ts`
- `./node_modules/.bin/svelte-kit sync && ./node_modules/.bin/svelte-check --tsconfig ./tsconfig.json` from `apps/web`
