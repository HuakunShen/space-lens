import test from "ava";

import { buildDirectoryTree, getLargestNodes } from "../index.js";

test("buildDirectoryTree", (t) => {
  const result = buildDirectoryTree([process.cwd()], false);
  t.is(result.length, 1);
  // console.log(result);
  const largestNodes = getLargestNodes(result, 10);
  // console.log(largestNodes);
  t.is(largestNodes.children.length, 10);
});
