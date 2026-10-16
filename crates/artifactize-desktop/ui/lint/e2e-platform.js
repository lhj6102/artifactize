// Desktop E2E portability: everything that differs between operating systems lives in
// e2e/platform.ts. Every other E2E file uses that layer instead of Node modules that reach
// the OS (files, processes, networks, databases), process values, native path joining or
// OS-specific literals. Only modules that behave the same everywhere stay allowed.
import { builtinModules } from "node:module";

const portable = new Set(["assert", "assert/strict", "buffer", "crypto", "events", "test", "timers/promises", "util"]);
const modules = builtinModules.filter((name) => !name.startsWith("_") && !portable.has(name.replace(/^node:/, "")));
const message = "OS-dependent: use the portable function of e2e/platform.ts";

export default function e2ePlatform(files, platform) {
  return [{
    files,
    ignores: [platform],
    rules: {
      "no-restricted-imports": ["error", {
        paths: [...new Set(modules.flatMap((name) => name.startsWith("node:") ? [name] : [name, `node:${name}`]))]
          .map((name) => ({ name, message })),
      }],
      "no-restricted-properties": ["error",
        ...["platform", "env", "execPath", "kill"].map((property) => ({ object: "process", property, message })),
      ],
      "no-restricted-syntax": ["error",
        { selector: "MemberExpression[object.type='MetaProperty'][property.name=/^(dirname|filename|url)$/]", message },
        { selector: "Literal[value=/\\\\/]", message: `${message} (a backslash is a Windows path separator)` },
        { selector: "TemplateElement[value.cooked=/\\\\/]", message: `${message} (a backslash is a Windows path separator)` },
        { selector: "Literal[value=/^(win32|darwin|linux)$/]", message },
        { selector: "Literal[value=/\\.(exe|cmd|bat)$/i]", message },
      ],
    },
  }];
}
