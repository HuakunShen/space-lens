# Space Lens · Static Product Website

This is the independent public-facing website, not the real local/Cloudflare workbench in `apps/web`.

The site runs SvelteKit in static prerender mode and imports **production Svelte components directly from `@space-lens/web-ui`**. No copied chart implementation or screenshot masquerades as an interactive demo. The landing page embeds real `DiskChart`, `ChildList`, `MemoryPressureCard`, `MemoryBreakdownBar`, and `ProcessTable` components.

All data originates from the deterministic, validated `src/lib/demo-fixture.ts` module. It is fictional and never invokes a scan, requests local permissions, connects to a host, cleans files, or quits processes. Demo selection and navigation are client-memory only.

## Commands

From repository root:

```bash
corepack yarn install
corepack yarn workspace @space-lens/site dev
corepack yarn workspace @space-lens/site test
corepack yarn workspace @space-lens/site check
SPACE_LENS_SITE_BASE='' GITHUB_PAGES=true corepack yarn workspace @space-lens/site build
```

Production is published at **https://space-lens.huakun.tech/**, a custom domain served from `/`.
The workflow sets `SPACE_LENS_SITE_BASE=''` explicitly, so links, JavaScript, CSS, and fonts resolve from the domain root.
For a project Pages URL such as `huakunshen.github.io/space-lens/`, omit the override and build with `GITHUB_PAGES=true` instead.
GitHub Pages publishes `apps/site/build`. The separate Cloudflare workbench and desktop app remain unchanged.

Pages deployment is configured in `.github/workflows/site.yml`; the repository's Pages source must be set to **GitHub Actions** for the workflow to publish successfully.
