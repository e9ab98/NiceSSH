#!/usr/bin/env bash
#
# scripts/release.sh — cut a new NiceSSH release from your local machine.
#
# Behavior (see docs/superpowers/specs/2026-06-25-release-script-design.md):
#   1. fetch --tags from origin
#   2. verify working tree clean, on main, jq present
#   3. derive next version = last v*.*.* tag + 1 patch (or $INIT if no tags)
#   4. rewrite 4 files in place (package.json, tauri.conf.json, Cargo.toml, release-please-manifest.json)
#   5. print a preview block
#   6. with --dry-run, exit 0; else commit + push main + push tag
#
# Exit codes:
#   0  success (or dry-run preview)
#   1  local-state error (dirty tree, wrong branch, jq missing, last tag unparseable, etc.)
#   2  remote-state conflict (push rejected — developer must git pull --rebase or git fetch --tags)
#   3  bootstrap needed (no v* tags found; rerun with INIT=v0.1.0)
#
set -euo pipefail

DRY_RUN=0
if [ "${1:-}" = "--dry-run" ]; then
  DRY_RUN=1
elif [ -n "${1:-}" ]; then
  echo "::error::Unknown argument: $1" >&2
  echo "Usage: scripts/release.sh [--dry-run]" >&2
  exit 1
fi

# ---- preflight ----------------------------------------------------------------

if ! command -v jq >/dev/null 2>&1; then
  echo "::error::jq is required but not installed. Install with: brew install jq" >&2
  exit 1
fi

if ! command -v git >/dev/null 2>&1; then
  echo "::error::git is required but not on PATH" >&2
  exit 1
fi

# Always operate from the repo root so relative paths are stable.
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" != "main" ]; then
  echo "::error::Must be on 'main' (currently on '${BRANCH}'). Run: git checkout main" >&2
  exit 1
fi

if [ -n "$(git status --porcelain)" ]; then
  echo "::error::Working tree is not clean. Commit or stash your changes first:" >&2
  git status --short >&2
  exit 1
fi

# Sync the local view of tags. This is the only network call before push.
git fetch --tags --force >/dev/null

# ---- derive next version ------------------------------------------------------

derive_next_version() {
  local last_tag
  last_tag=$(git tag -l "v*.*.*" --sort=-v:refname | head -1 || true)
  if [ -z "$last_tag" ]; then
    if [ -n "${INIT:-}" ]; then
      # INIT may be "v0.3.4" or "0.3.4" — caller always prefixes "v"
      echo "${INIT#v}"
      return 0
    fi
    echo "::error::No v* tags found locally; bootstrap with INIT=v0.1.0 scripts/release.sh" >&2
    exit 3
  fi
  if [[ "$last_tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    local major="${BASH_REMATCH[1]}"
    local minor="${BASH_REMATCH[2]}"
    local patch="${BASH_REMATCH[3]}"
    echo "${major}.${minor}.$((patch + 1))"
    return 0
  fi
  echo "::error::Last tag ${last_tag} is not vMAJOR.MINOR.PATCH; refusing to bump." >&2
  exit 1
}

VERSION="$(derive_next_version)"
TAG="v${VERSION}"
COMMIT_MSG="chore(release): v${VERSION}"

# Read current values so the preview can show "before → after".
read_pkg_version()   { jq -r '.version' package.json; }
read_tauri_version() { jq -r '.version' src-tauri/tauri.conf.json; }
read_cargo_version() {
  awk '/^\[package\]/{in_pkg=1; next} /^\[/{in_pkg=0} in_pkg && /^version[[:space:]]*=/{print; exit}' \
    src-tauri/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/'
}
read_manifest_version() { jq -r '."src-tauri"' release-please-manifest.json; }

OLD_PKG="$(read_pkg_version)"
OLD_TAURI="$(read_tauri_version)"
OLD_CARGO="$(read_cargo_version)"
OLD_MANIFEST="$(read_manifest_version)"

# Sanity: all 4 must currently agree (catches a half-bumped state from a prior
# failed run or from tag.yml's known `git add` bug that left package.json behind).
if [ "$OLD_PKG" != "$OLD_TAURI" ] || [ "$OLD_TAURI" != "$OLD_CARGO" ] || [ "$OLD_TAURI" != "$OLD_MANIFEST" ]; then
  echo "::error::Version drift detected across the 4 source files:" >&2
  echo "  package.json:                ${OLD_PKG}" >&2
  echo "  tauri.conf.json:             ${OLD_TAURI}" >&2
  echo "  Cargo.toml:                  ${OLD_CARGO}" >&2
  echo "  release-please-manifest.json:${OLD_MANIFEST}" >&2
  echo "Fix manually before running release.sh, or accept a known-bad state by editing each file to match." >&2
  exit 1
fi

# ---- file rewrites ------------------------------------------------------------

# 1. package.json (jq in-place)
rewrite_pkg() {
  local tmp
  tmp="$(mktemp)"
  jq --arg v "$VERSION" '.version = $v' package.json > "$tmp"
  mv "$tmp" package.json
}

# 2. src-tauri/tauri.conf.json (jq in-place)
rewrite_tauri() {
  local tmp
  tmp="$(mktemp)"
  jq --arg v "$VERSION" '.version = $v' src-tauri/tauri.conf.json > "$tmp"
  mv "$tmp" src-tauri/tauri.conf.json
}

# 3. src-tauri/Cargo.toml (awk; POSIX, portable to macOS BSD sed)
#    Rewrites the first `^version = "..."` inside the [package] section.
rewrite_cargo() {
  local tmp
  tmp="$(mktemp)"
  awk -v ver="$VERSION" '
    BEGIN { in_pkg = 0; done_ = 0 }
    /^\[package\]/ { in_pkg = 1; print; next }
    /^\[/            { in_pkg = (done_ ? in_pkg : 0) }
    in_pkg && !done_ && /^version[[:space:]]*=/ {
      sub(/^version[[:space:]]*=[[:space:]]*".*"/, "version = \"" ver "\"")
      done_ = 1
    }
    { print }
  ' src-tauri/Cargo.toml > "$tmp"
  mv "$tmp" src-tauri/Cargo.toml
}

# 4. release-please-manifest.json (jq in-place; preserves key shape)
rewrite_manifest() {
  local tmp
  tmp="$(mktemp)"
  jq --arg v "$VERSION" '{ "src-tauri": $v }' release-please-manifest.json > "$tmp"
  mv "$tmp" release-please-manifest.json
}

# ---- preview ------------------------------------------------------------------

print_preview() {
  cat <<EOF
── Release preview ──
next version:  ${VERSION}
next tag:      ${TAG}
files changed:
  package.json                              (${OLD_PKG} → ${VERSION})
  src-tauri/tauri.conf.json                 (${OLD_TAURI} → ${VERSION})
  src-tauri/Cargo.toml                      (${OLD_CARGO} → ${VERSION})
  release-please-manifest.json              ("src-tauri": "${OLD_MANIFEST}" → "${VERSION}")
commit message: ${COMMIT_MSG}
────────────────────
EOF
}

print_preview

# ---- execute (or stop on --dry-run) -------------------------------------------

if [ "$DRY_RUN" = "1" ]; then
  echo "(dry-run; no files written, no commits, no pushes)"
  echo ""
  echo "Re-run without --dry-run to perform the release:"
  echo "  scripts/release.sh"
  exit 0
fi

# Sanity: refuse to push a tag that already exists on the remote.
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "::error::Tag ${TAG} already exists locally; refusing to push a duplicate." >&2
  echo "  If this is correct, run: git push origin ${TAG}" >&2
  echo "  If not, delete the local tag first: git tag -d ${TAG}" >&2
  exit 1
fi

# We use a trap to roll back partial edits if anything fails mid-flight.
rollback() {
  echo "::error::Release failed mid-flight; rolling back any partial file edits." >&2
  git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json 2>/dev/null || true
}
trap rollback ERR

# Rewrite in fixed order; trap will roll back if any of them error.
rewrite_pkg
rewrite_tauri
rewrite_cargo
rewrite_manifest

# From here on we don't want the rollback trap — the commit is the next step.
trap - ERR

# Show the diff for one last visual check before we commit.
echo ""
echo "── git diff (4 files) ──"
git --no-pager diff -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
echo "────────────────────────"
echo ""

# Confirm with the developer. Set ASSUME_YES=1 to skip (e.g. for CI wrapping).
if [ "${ASSUME_YES:-0}" != "1" ]; then
  read -r -p "Commit and push? [y/N] " ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *)
      echo "Aborted by user. Rolling back file edits."
      git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
      exit 1
      ;;
  esac
fi

git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json

# Commit. If there's nothing to commit (e.g. the version was already on disk
# from a previous failed run), fail loudly rather than silently no-op.
if ! git diff --cached --quiet; then
  git commit -m "$COMMIT_MSG"
else
  echo "::error::No changes staged for commit; aborting before push." >&2
  git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
  exit 1
fi

# Push main. If rejected, exit 2 so the developer can rebase.
echo "→ Pushing main…"
if ! git push origin HEAD:main; then
  echo "::error::git push origin main was rejected (likely a remote commit you don't have locally)." >&2
  echo "  Resolve manually:" >&2
  echo "    git pull --rebase" >&2
  echo "    scripts/release.sh   # re-run" >&2
  echo "  Or, if you only need the tag pushed (the version commit is already on main):" >&2
  echo "    git push origin ${TAG}" >&2
  exit 2
fi

# Push the tag. If rejected, exit 2 (e.g. someone else just pushed the same tag).
echo "→ Pushing tag ${TAG}…"
if ! git push origin "refs/tags/${TAG}"; then
  echo "::error::git push origin ${TAG} was rejected." >&2
  echo "  Likely causes: tag already exists on remote, or you lack push-permission on tags." >&2
  echo "  Local tag is created but NOT pushed. To retry: git push origin ${TAG}" >&2
  exit 2
fi

echo ""
echo "✅ Released ${TAG}"
echo "→ Build & Publish Release will run on the new tag."
echo "→ Watch progress: https://github.com/e9ab98/NiceSSH/actions"
