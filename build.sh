#!/bin/sh
# Builds the front-end into dist/ (deno bundle, no Node required).
set -e
cd "$(dirname "$0")"
DENO="${DENO:-$(command -v deno || echo /opt/homebrew/bin/deno)}"
rm -rf dist && mkdir -p dist
MINIFY="--minify"
[ "$1" = "--dev" ] && MINIFY="--sourcemap=inline"
"$DENO" bundle --config ui/deno.json --platform browser --format esm --code-splitting --outdir dist $MINIFY ui/main.ts
cp ui/index.html ui/styles.css dist/
cp icons/icon.png dist/icon.png
echo "built: $(du -sh dist | cut -f1)"
