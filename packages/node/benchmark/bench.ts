import { Bench } from 'tinybench'

import { scanDirectory } from '../index.js'

const b = new Bench()

b.add('scan current directory tree', () => {
  scanDirectory({
    directories: [process.cwd()],
    ignoreHidden: true,
    fullPath: false,
    respectGitignore: true,
    ignoredMode: 'summarize',
  })
})

await b.run()

console.table(b.table())
