import test from "ava";

import { buildDirectoryTree, getLargestNodes } from "../index.js";

test("buildDirectoryTree", (t) => {
  const result = buildDirectoryTree({
    directories: [process.cwd()],
    ignoreHidden: false,
    fullPath: true,
  });
  t.is(result.length, 1);
  console.dir(result, { depth: null });
  const largestNodes = getLargestNodes(result, 10);
  console.dir(largestNodes, { depth: null });
  t.is(largestNodes.children.length, 10);
});
