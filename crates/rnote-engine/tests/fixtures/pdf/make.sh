#!/usr/bin/env bash
# Regenerates the Pdf fixtures. Run inside `nix develop`.
set -euo pipefail
cd "$(dirname "$0")"
magick -size 1240x1754 gradient:white-gray40 -quality 85 scan-page.jpg
typst compile vector.typ vector.pdf
typst compile scan.typ scan.pdf
typst compile mixed.typ mixed.pdf
