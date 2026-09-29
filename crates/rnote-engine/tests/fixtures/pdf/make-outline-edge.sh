#!/usr/bin/env bash
# Writes outline-edge.pdf, whose outline uses forms Typst does not emit: a named and a string
# destination, a GoTo action, a Utf-16 title, an item pointing to a Url and an item that loops.
set -euo pipefail
cd "$(dirname "$0")"

objects=(
  '<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /Dests 7 0 R /Names 8 0 R >>'
  '<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Outlines /First 11 0 R /Last 17 0 R /Count 6 >>'
  '<< /N1 [4 0 R /Fit] >>'
  '<< /Dests 9 0 R >>'
  '<< /Kids [10 0 R] >>'
  '<< /Limits [(S1) (S1)] /Names [(S1) << /D [5 0 R /Fit] >>] >>'
  '<< /Title (Named) /Parent 6 0 R /Next 12 0 R /Dest /N1 >>'
  '<< /Title (Tree) /Parent 6 0 R /Prev 11 0 R /Next 13 0 R /Dest (S1) >>'
  '<< /Title (GoTo) /Parent 6 0 R /Prev 12 0 R /Next 15 0 R /First 14 0 R /Last 14 0 R /Count 1 /A << /S /GoTo /D [3 0 R /Fit] >> >>'
  '<< /Title <FEFF7B2C00317AE0> /Parent 13 0 R /Dest [3 0 R /XYZ null null null] >>'
  '<< /Title (Link) /Parent 6 0 R /Prev 13 0 R /Next 17 0 R /First 16 0 R /Last 16 0 R /Count 1 /A << /S /URI /URI (https://example.com) >> >>'
  '<< /Title (Promoted) /Parent 15 0 R /Dest [4 0 R /Fit] >>'
  '<< /Title (Loop) /Parent 6 0 R /Prev 15 0 R /Next 17 0 R /Dest [5 0 R /Fit] >>'
)

# Every byte is Ascii, so string lengths are byte offsets.
pdf=$'%PDF-1.7\n'
offsets=()
for i in "${!objects[@]}"; do
  offsets+=("${#pdf}")
  pdf+="$((i + 1)) 0 obj"$'\n'"${objects[$i]}"$'\nendobj\n'
done
xref_start=${#pdf}
size=$((${#objects[@]} + 1))
pdf+="xref"$'\n'"0 $size"$'\n'"0000000000 65535 f "$'\n'
for offset in "${offsets[@]}"; do
  printf -v entry '%010d 00000 n \n' "$offset"
  pdf+=$entry
done
pdf+="trailer"$'\n'"<< /Size $size /Root 1 0 R >>"$'\n'"startxref"$'\n'"$xref_start"$'\n'"%%EOF"$'\n'
printf '%s' "$pdf" > outline-edge.pdf
