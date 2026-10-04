#!/usr/bin/env bash
# Build the artifactize.dev site into website/dist.
set -euo pipefail
cd "$(dirname "$0")"
rm -rf dist
mkdir -p dist
cp -R landing/. dist/
echo "built $(find dist -type f | wc -l) files into website/dist"
