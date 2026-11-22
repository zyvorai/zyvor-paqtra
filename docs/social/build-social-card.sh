#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# Render the social cards with headless Chrome:
#   paqtra-share-card.html  (1200x630) -> paqtra-share-card.png   README hero + website og:image
#   paqtra-social-card.html (1600x900) -> paqtra-social-card.jpg  LinkedIn / X
# Needs Google Chrome; the JPEG step uses macOS `sips`. Nothing is installed.
#   ./docs/social/build-social-card.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[[ -x "$CHROME" ]] || { echo "Google Chrome not found (set CHROME=...)" >&2; exit 1; }

shot() { # html width height out.png
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
    --window-size="$2,$3" --screenshot="$4" "file://$HERE/$1" >/dev/null 2>&1
}

shot paqtra-share-card.html 1200 630 "$HERE/paqtra-share-card.png"
echo "wrote $HERE/paqtra-share-card.png ($(du -k "$HERE/paqtra-share-card.png" | cut -f1) KB)"

PNG="$(mktemp "${TMPDIR:-/tmp}/paqtra-card.XXXXXX.png")"
trap 'rm -f "$PNG"' EXIT
for card in paqtra-social-card; do
  shot "$card.html" 1600 900 "$PNG"
  sips -s format jpeg -s formatOptions 92 "$PNG" --out "$HERE/$card.jpg" >/dev/null
  echo "wrote $HERE/$card.jpg ($(du -k "$HERE/$card.jpg" | cut -f1) KB)"
done
