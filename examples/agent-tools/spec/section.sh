#!/bin/sh
# Plain-protocol Agent tool: print the "## TITLE" section of spec.md.
#
# artifactize substitutes the validated "title" argument for {title} in argv,
# runs this script from the spec folder with empty stdin, and returns stdout as
# one text block. A nonzero exit marks that text as a tool error.
title=$1
found=no
while IFS= read -r line; do
  case $line in
    "## "*)
      [ "$found" = yes ] && exit 0
      [ "${line#"## "}" = "$title" ] && found=yes
      ;;
  esac
  [ "$found" = yes ] && printf '%s\n' "$line"
done < spec.md
if [ "$found" = no ]; then
  printf 'No section titled "%s". Sections:\n' "$title"
  grep '^## ' spec.md
  exit 1
fi
