import adapter from '@sveltejs/adapter-static'

const githubPages = process.env.GITHUB_PAGES === 'true'
// Custom domains serve at "/"; the github.io project fallback serves at "/space-lens".
// An explicitly empty override is intentional and must not fall back.
const siteBase = process.env.SPACE_LENS_SITE_BASE ?? (githubPages ? '/space-lens' : '')

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    paths: { base: siteBase },
    adapter: adapter({ pages: 'build', assets: 'build', strict: true }),
  },
}

export default config
