#!/bin/sh
set -eu
[ -f instances.json ]
actual=$(cat "$1")
printf '%s\n' "$actual"
[ "$actual" = "$2" ]
