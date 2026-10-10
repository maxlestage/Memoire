#!/usr/bin/env bash
# Builds the website for a static host without server-side rewrites, such as
# GitHub Pages, where it is served from a sub-path (https://<user>.github.io/<repo>/).
#
#   site/build-static.sh <base-path> [site-url] [out-dir]
#   site/build-static.sh /Memoire https://maxlestage.github.io/Memoire
#
# <base-path> is the path the site lives under ("" or "/" for a domain root).
# [site-url] is the public address, used for the social preview image.
# [out-dir] defaults to site/dist-static.
#
# Every page is prerendered at build time (`cargo run -- prerender`, see
# src/prerender.rs) into its own HTML file (`docs/models.html` and
# `docs/models/index.html`), so it shows before the WebAssembly app has even
# downloaded, and deep links and refreshes answer 200; any other path gets
# 404.html, prerendered "page not found".
set -euo pipefail

base="${1:-}"
base="${base%/}"
site_url="${2:-}"
site_url="${site_url%/}"
cd "$(dirname "$0")"
out="${3:-dist-static}"

MEMOIRE_BASE_PATH="$base" trunk build --release --public-url "$base/" --dist "$out"

if [ -n "$site_url" ]; then
  sed -i "s#content=\"/og-image.png\"#content=\"$site_url/og-image.png\"#g" "$out/index.html"
fi

# Native build of the same crate: renders every page into a copy of index.html.
MEMOIRE_BASE_PATH="$base" cargo run --quiet -- prerender "$out"
touch "$out/.nojekyll"

echo "Static site in $(cd "$out" && pwd) (base path: '${base:-/}')"
