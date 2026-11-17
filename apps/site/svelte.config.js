import adapter from '@sveltejs/adapter-static'

const githubPages = process.env.GITHUB_PAGES === 'true'

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    paths: { base: githubPages ? '/space-lens' : '' },
    adapter: adapter({ pages: 'build', assets: 'build', strict: true }),
  },
}

export default config
