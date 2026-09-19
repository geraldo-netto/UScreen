#!/usr/bin/env bash
# Build and publish a GitHub release with the complete set of files.
#
# The set is checked before any release API writes: a release with a
# file missing is exactly how the PKGBUILD got left out of 1.1.0, so this
# script would rather fail than publish half a release.
#
# Needs GH_TOKEN in the environment (never passed on a command line) and the
# uscreen-build container for portable binaries.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^VERSION = //p' Makefile)"
REPO="geraldo-netto/UScreen"
# The date that goes on the website and in CITATION.cff. Today unless the
# release was dated in advance.
RELEASE_DATE="${RELEASE_DATE:-$(date +%F)}"
: "${GH_TOKEN:?set GH_TOKEN first}"

NOTES="${1:-}"
[ -n "$NOTES" ] && [ -f "$NOTES" ] || { echo "usage: $0 <release-notes.md>  (tag v$VERSION must exist on origin)"; exit 1; }

git rev-parse --verify "refs/tags/v$VERSION" >/dev/null 2>&1 || { echo "!! tag v$VERSION does not exist — create and push it first"; exit 1; }

# Match the immutable tag object too: annotated tags must not be replaced even
# when their peeled commit is unchanged. ls-remote does not mutate local refs.
check_release_refs() {
  local tag="refs/tags/v$VERSION" head commit local_tag remote_tag
  head=$(git rev-parse HEAD)
  commit=$(git rev-parse "$tag^{commit}")
  [ "$head" = "$commit" ] || { echo "!! HEAD differs from $tag"; exit 1; }
  local_tag=$(git rev-parse "$tag")
  remote_tag=$(git ls-remote --exit-code --refs origin "$tag" | awk '{print $1}') \
    || { echo "!! $tag missing or unreadable on origin"; exit 1; }
  [ "$local_tag" = "$remote_tag" ] || { echo "!! local and origin $tag differ"; exit 1; }
}
check_release_refs
RELEASE_COMMIT=$(git rev-parse HEAD)
RELEASE_TAG_OBJECT=$(git rev-parse "refs/tags/v$VERSION")

check_release_sources() {
  [ "$(git rev-parse HEAD)" = "$RELEASE_COMMIT" ] \
    && [ "$(git rev-parse "refs/tags/v$VERSION")" = "$RELEASE_TAG_OBJECT" ] \
    || { echo "!! release source/tag changed during the build"; exit 1; }
  [ -z "$(git status --porcelain --untracked-files=all)" ] \
    || { echo "!! uncommitted changes — release sources must remain clean"; exit 1; }
}

# Every place that repeats the version has to agree with the Makefile before
# anything is built, so the public page never advertises the previous release.
require_metadata_line() {
  local file="$1" expected="$2"
  sed 's/^[[:space:]]*//;s/[[:space:]]*$//' "$file" | grep -Fx -- "$expected" >/dev/null \
    || { echo "!! $file has no exact '$expected' entry"; exit 1; }
}
for f in host/Cargo.toml gui/Cargo.toml common/Cargo.toml; do
  require_metadata_line "$f" "version = \"$VERSION\""
done
require_metadata_line android/app/build.gradle.kts "versionName = \"$VERSION\""
require_metadata_line packaging/arch/PKGBUILD "pkgver=$VERSION"
require_metadata_line CHANGELOG.md "## $VERSION — $RELEASE_DATE"
./scripts/update-release-metadata.sh --check "$VERSION" "$RELEASE_DATE" \
  || { echo "!! website/citation metadata is stale — run 'scripts/update-release-metadata.sh $VERSION $RELEASE_DATE', commit, then publish again"; exit 1; }

check_release_sources

./scripts/build-release.sh
./packaging/build-packages.sh

# The complete set. Add here when a release gains a file; the check below
# keeps every future release honest about it.
ASSETS=(
  "dist/uscreen-$VERSION-linux-x86_64.tar.gz:application/gzip"
  "dist/uscreen_${VERSION}_amd64.deb:application/vnd.debian.binary-package"
  "dist/uscreen-$VERSION-1.x86_64.rpm:application/x-rpm"
  "dist/uscreen-$VERSION-PKGBUILD.tar.gz:application/gzip"
  "dist/uscreen-$VERSION/uscreen.apk:application/vnd.android.package-archive"
)
for a in "${ASSETS[@]}"; do
  f="${a%%:*}"
  [ -f "$f" ] || { echo "!! missing: $f — not publishing an incomplete release"; exit 1; }
done
echo "All $(( ${#ASSETS[@]} )) files present."

python3 scripts/verify-release-apk.py "dist/uscreen-$VERSION/uscreen.apk"

# Checksums for everything above, published alongside.
( cd dist && sha256sum "uscreen-$VERSION-linux-x86_64.tar.gz" "uscreen_${VERSION}_amd64.deb" \
    "uscreen-$VERSION-1.x86_64.rpm" "uscreen-$VERSION-PKGBUILD.tar.gz" > SHA256SUMS \
  && cp "uscreen-$VERSION/uscreen.apk" . && sha256sum uscreen.apk >> SHA256SUMS && rm uscreen.apk )
ASSETS+=("dist/SHA256SUMS:text/plain")

check_release_refs
# T263: HEAD/tag equality alone cannot detect edits to build inputs. Refuse
# publication after staged, unstaged or untracked source changes. Ignored
# outputs and externally supplied SDK/signing inputs follow development.md.
check_release_sources

# The release title is what shows up in feeds and search results, so it says
# what the project is rather than just the tag.
TITLE="UScreen $VERSION — USB second monitor for Linux with S Pen support"

python3 - "$NOTES" "$VERSION" "$TITLE" "$REPO" "${ASSETS[@]}" <<'PY'
import hashlib, json, os, pathlib, sys, urllib.parse, urllib.request

notes, version, title, repo, *assets = sys.argv[1:]
api = f"https://api.github.com/repos/{repo}/releases"
headers = {"Authorization": "Bearer " + os.environ["GH_TOKEN"],
           "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}

def request(url, method="GET", body=None, kind="application/json", size=None):
    extra = {"Content-Type": kind}
    if size is not None:
        extra["Content-Length"] = str(size)
    req = urllib.request.Request(url, data=body, method=method, headers=headers | extra)
    with urllib.request.urlopen(req, timeout=120) as response:
        return json.load(response)

def json_request(url, method, body):
    return request(url, method, json.dumps(body).encode())

def verify(asset, expected):
    if not isinstance(asset, dict) or any(asset.get(k) != v for k, v in expected.items()):
        raise RuntimeError(f"asset verification failed: {expected['name']}")

# Hash every local file before uploading, including the checksum manifest.
expected = {}
for asset in assets:
    path, kind = asset.rsplit(":", 1)
    file = pathlib.Path(path)
    with file.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    expected[file.name] = {"name": file.name, "size": file.stat().st_size,
                           "state": "uploaded", "digest": "sha256:" + digest}

release = json_request(api, "POST", {"tag_name": "v" + version, "name": title,
                       "body": pathlib.Path(notes).read_text(), "draft": True})
if release.get("draft") is not True or not isinstance(release.get("id"), int):
    raise RuntimeError("API did not create a draft release")
release_url = f"{api}/{release['id']}"
print(f"Draft release id: {release['id']}; failures leave it unpublished.")
for asset in assets:
    path, kind = asset.rsplit(":", 1)
    name = pathlib.Path(path).name
    url = (f"https://uploads.github.com/repos/{repo}/releases/{release['id']}/assets"
           f"?name={urllib.parse.quote(name)}")
    with open(path, "rb") as data:
        uploaded = request(url, "POST", data, kind, expected[name]["size"])
    verify(uploaded, expected[name])
    print("  verified", name)

# Read back the server's complete asset inventory and SHA-256 digests. A 200
# error body, starter asset, truncated upload or missing asset cannot publish.
remote = request(release_url + "/assets?per_page=100")
if not isinstance(remote, list) or len(remote) != len(expected):
    raise RuntimeError("release asset set is incomplete")
if {asset.get("name") for asset in remote} != set(expected):
    raise RuntimeError("release asset names differ")
for asset in remote:
    verify(asset, expected[asset["name"]])
published = json_request(release_url, "PATCH", {"draft": False})
if published.get("draft") is not False or published.get("tag_name") != "v" + version:
    raise RuntimeError("release publication was not confirmed")
PY
echo "✓ https://github.com/$REPO/releases/tag/v$VERSION"
