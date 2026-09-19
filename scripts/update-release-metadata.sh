#!/usr/bin/env bash
# Set — or, with --check, verify — release-candidate metadata. Preparing files
# does not establish that a GitHub release has been published:
#
#   docs/index.html   source version, candidate link/status, package filenames,
#                     modification date; publication/download metadata stays null
#   docs/llms.txt     source version, candidate status and metadata update date
#   docs/sitemap.xml  every <lastmod>
#   CITATION.cff      source version; no unverified release date
#
# Every pattern must match at least once, so a rewrite of one of those files
# that drops a marker makes this script fail instead of silently leaving an
# old version on the public page. publish-release.sh runs the --check form
# before it publishes anything.
#
#   scripts/update-release-metadata.sh 1.2.0 2026-09-15
#   scripts/update-release-metadata.sh --check 1.2.0 2026-09-15
set -euo pipefail
cd "$(dirname "$0")/.."

usage() { echo "usage: $0 [--check] <version> <YYYY-MM-DD>" >&2; exit 2; }
CHECK=0
if [ "${1:-}" = "--check" ]; then CHECK=1; shift; fi
[ $# -eq 2 ] || usage
VERSION="$1"; DATE="$2"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "!! version must look like 1.2.3, got '$VERSION'" >&2; exit 2; }
[[ "$DATE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] && date -d "$DATE" >/dev/null 2>&1 \
  || { echo "!! date must be a real YYYY-MM-DD, got '$DATE'" >&2; exit 2; }

python3 - "$CHECK" "$VERSION" "$DATE" <<'PY'
import re, sys

check, version, date = sys.argv[1] == "1", sys.argv[2], sys.argv[3]
NUM = r"[0-9]+\.[0-9]+\.[0-9]+"
DAY = r"[0-9]{4}-[0-9]{2}-[0-9]{2}"

# file -> [(pattern, replacement)]; every pattern must hit at least once
RULES = {
    "docs/index.html": [
        (rf'"softwareVersion": "{NUM}"', f'"softwareVersion": "{version}"'),
        (r'"datePublished": null', '"datePublished": null'),
        (r'"downloadUrl": null', '"downloadUrl": null'),
        (rf'"dateModified": "{DAY}"', f'"dateModified": "{date}"'),
        (r'<a id="release-download"[^>]*>[^<]*</a>',
         f'<a id="release-download" class="btn primary" href="https://github.com/geraldo-netto/UScreen/releases/tag/v{version}">Download {version}</a>'),
        (r'<p id="release-status">.*?</p>',
         f'<p id="release-status">Release candidate {version}; check the release page for publication and available files. Build from source if it is not published.</p>'),
        (rf'<span id="source-version">{NUM}</span>', f'<span id="source-version">{version}</span>'),
        (rf'<time datetime="{DAY}">{DAY}</time>', f'<time datetime="{date}">{date}</time>'),
        (rf"uscreen-{NUM}-x86_64\.AppImage", f"uscreen-{version}-x86_64.AppImage"),
        (rf"uscreen-{NUM}-1\.x86_64\.rpm", f"uscreen-{version}-1.x86_64.rpm"),
        (rf"uscreen-{NUM}-PKGBUILD\.tar\.gz", f"uscreen-{version}-PKGBUILD.tar.gz"),
        (rf"uscreen-{NUM}-linux-x86_64\.tar\.gz", f"uscreen-{version}-linux-x86_64.tar.gz"),
    ],
    "docs/llms.txt": [
        (rf"Current version: {NUM} \(unreleased\)", f"Current version: {version} (unreleased)"),
        (rf"Metadata updated: {DAY}", f"Metadata updated: {date}"),
        (r"^Release status: .*", f"Release status: candidate {version}; check the releases page for publication and available files."),
    ],
    "docs/sitemap.xml": [
        (rf"<lastmod>{DAY}</lastmod>", f"<lastmod>{date}</lastmod>"),
    ],
    "CITATION.cff": [
        (rf'^version: "{NUM}"', f'version: "{version}"'),
        (r'^# date-released: unpublished', '# date-released: unpublished'),
    ],
}

# Keep edits in memory until every input file and required marker is valid.
stale, broken = {}, []
for path, rules in RULES.items():
    with open(path, encoding="utf-8") as f:
        original = f.read()
    text = original
    for pattern, repl in rules:
        text, n = re.subn(pattern, repl, text, flags=re.M)
        if n == 0:
            broken.append(f"{path}: no match for /{pattern}/")
    if text != original:
        stale[path] = text

if broken:
    print("!! metadata markers missing — the files were rewritten without them:", file=sys.stderr)
    for b in broken:
        print("   " + b, file=sys.stderr)
    sys.exit(1)
if check:
    if stale:
        print(f"!! release metadata is not {version} / {date} in: " + ", ".join(stale), file=sys.stderr)
        print(f"   run: scripts/update-release-metadata.sh {version} {date}", file=sys.stderr)
        sys.exit(1)
    print(f"release metadata is {version} / {date} everywhere.")
elif not stale:
    print(f"already {version} / {date} everywhere; nothing to do.")
else:
    for path, text in stale.items():
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"updated {path}")
PY
