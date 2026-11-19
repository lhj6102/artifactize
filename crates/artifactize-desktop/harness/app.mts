// Human launch tool, independent of the headless WDIO service and its fixture state.
import { prepareApp, workspace } from "../ui/e2e/build.ts";
import { removeTree, runVisible } from "../ui/e2e/portable.ts";

try {
  const { root, app } = await prepareApp();
  try {
    console.log(`Launching the desktop app on ${workspace} with your own artifactize state`);
    process.exitCode = await runVisible(app, ["--repo", workspace], process.env, workspace);
  } finally {
    removeTree(root);
    console.log(`App closed; removed ${root}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
