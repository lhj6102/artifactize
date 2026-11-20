// Desktop E2E portability (E2E-PLATFORM): whatever differs between operating systems lives in
// e2e/platform.ts. Every other E2E file may use portable Node I/O (files, processes, sockets,
// paths joined by node:path), but never names an OS or branches on one: no OS identifiers, no
// OS-specific separators or executable suffixes, no Windows- or POSIX-only path flavours.
const message = "OS-specific: handle this difference in e2e/platform.ts";

export default function e2ePlatform(files, platform) {
  return [{
    files,
    ignores: [platform],
    rules: {
      "no-restricted-properties": ["error",
        { object: "process", property: "platform", message },
        { object: "os", property: "platform", message },
        { object: "os", property: "type", message },
        { object: "os", property: "EOL", message },
        { object: "path", property: "win32", message },
        { object: "path", property: "posix", message },
        { object: "path", property: "sep", message },
        { object: "path", property: "delimiter", message },
      ],
      "no-restricted-imports": ["error", {
        paths: ["node:os", "os"].map((name) => ({ name, importNames: ["platform", "type", "EOL"], message }))
          .concat(["node:path", "path"].map((name) => ({ name, importNames: ["win32", "posix", "sep", "delimiter"], message }))),
      }],
      "no-restricted-syntax": ["error",
        { selector: "Literal[value=/\\\\/]", message: `${message} (a backslash is a Windows path separator)` },
        { selector: "TemplateElement[value.cooked=/\\\\/]", message: `${message} (a backslash is a Windows path separator)` },
        { selector: "Literal[value=/^(win32|darwin|linux)$/]", message },
        { selector: "Literal[value=/\\.(exe|cmd|bat)$/i]", message },
      ],
    },
  }];
}
