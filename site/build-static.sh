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
# Every page of the app gets its own copy of index.html (`docs/models.html` and
# `docs/models/index.html`), so deep links and refreshes answer 200; any other
# path gets 404.html, which boots the app on its "not found" page.
set -euo pipefail

base="${1:-}"
base="${base%/}"
site_url="${2:-}"
site_url="${site_url%/}"
cd "$(dirname "$0")"
out="${3:-dist-static}"
docs_dir="$(cd ../docs/en && pwd)"

MEMOIRE_BASE_PATH="$base" trunk build --release --public-url "$base/" --dist "$out"

cd "$out"
if [ -n "$site_url" ]; then
  sed -i "s#content=\"/og-image.png\"#content=\"$site_url/og-image.png\"#g" index.html
fi

routes="features performance playground docs"
for page in "$docs_dir"/*.md; do
  routes="$routes docs/$(basename "$page" .md)"
done
for route in $routes; do
  mkdir -p "$route"
  cp index.html "$route.html"
  cp index.html "$route/index.html"
done
cp index.html 404.html
touch .nojekyll

echo "Static site in $(pwd) (base path: '${base:-/}', $(echo $routes | wc -w) routes)"
