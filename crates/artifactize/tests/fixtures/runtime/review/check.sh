#!/bin/sh
set -eu
test "$(cat "$1")" = "scoped input"
test -z "${ARTIFACTIZE_TEST_SECRET+x}"
test "$PWD" = "$ARTIFACTIZE_WORKSPACE_DIR/review"
printf 'runtime output\n' > "$ARTIFACTIZE_OUTPUT_DIR/result.txt"
printf '\033[32mGREEN\033[0m scoped input\n'
printf 'runtime stderr\n' >&2
