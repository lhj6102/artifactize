// Human launch tool, independent of the headless WDIO service and its fixture state.
import { prepareApp, workspace } from "../ui/e2e/build.ts";
import { inheritedEnvironment, removeTree, runVisible, setExitCode } from "../ui/e2e/platform.ts";

const environment = inheritedEnvironment();
try {
  const { root, app } = await prepareApp();
  try {
    console.log(`Launching the desktop app on ${workspace} with your own artifactize state`);
    setExitCode(await runVisible(app, ["--repo", workspace], environment, workspace));
  } finally {
    removeTree(root);
    console.log(`App closed; removed ${root}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  setExitCode(1);
}
