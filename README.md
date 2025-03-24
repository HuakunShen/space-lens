# Space Lens

To Publish a new version:

```bash
npm version patch
git push --follow-tags
```

## Usage

```ts
import { buildDirectoryTree, getLargestNodes } from "space-lens";

const result = buildDirectoryTree({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: true, // set to false to get only the file name (less space)
});
console.dir(result, { depth: null });
const largestNodes = getLargestNodes(result, 10);
console.dir(largestNodes, { depth: null });
```
