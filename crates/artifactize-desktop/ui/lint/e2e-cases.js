// Desktop E2E cases (E2E-REAL-APP): a case reads the window and waits for it only through the
// suite's views, whose actions each wait for their visible result. No case scripts the page,
// reads painted styles or waits for time.
const message = "cases read and wait through e2e/views (E2E-REAL-APP)";

export default function e2eCases(files, level = "error") {
  return [{
    files,
    rules: {
      "no-restricted-properties": [level,
        ...["execute", "executeAsync", "waitUntil", "pause"].map((property) => ({ property, message })),
      ],
      "no-restricted-globals": [level,
        ...["getComputedStyle", "setTimeout", "setInterval", "document", "window"].map((name) => ({ name, message })),
      ],
    },
  }];
}
