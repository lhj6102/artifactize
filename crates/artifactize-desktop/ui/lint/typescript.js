// One TypeScript policy, scoped by each Artifact's own ESLint configuration.
import { fileURLToPath } from "node:url";
import tseslint from "typescript-eslint";

const tsconfigRootDir = fileURLToPath(new URL("../", import.meta.url));

export default function typescript(files) {
  return [
    ...tseslint.configs.recommended.map((config) => ({ ...config, files })),
    { files, languageOptions: { parserOptions: { tsconfigRootDir } } },
  ];
}
