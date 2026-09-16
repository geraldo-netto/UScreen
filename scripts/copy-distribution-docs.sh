#!/usr/bin/env bash
# One documentation/notice set for tarballs and native packages.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:?usage: copy-distribution-docs.sh DEST}"
mkdir -p "$DEST"
while IFS= read -r path; do
    cp -R "$ROOT/$path" "$DEST/"
done < "$ROOT/packaging/distribution-docs.txt"
