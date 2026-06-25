#!/usr/bin/env bash
#
# scripts/release.sh — cut a new NiceSSH release from your local machine.
#
# Behavior (see docs/superpowers/specs/2026-06-25-release-script-design.md):
#   1. fetch --tags from origin
#   2. verify working tree clean, on main, jq present
#   3. derive next version = last v*.*.* tag + 1 patch (or $INIT if no tags)
#   4. check 4 source files for version drift (exit 1 if they disagree)
#   5. print a preview block
#   6. prompt: [1] preview  [2] release  [3] preview-diff
#      - 1: exit 0 (no writes)
#      - 2: rewrite 4 files → git diff → y/N → commit + push main + push tag
#      - 3: rewrite 4 files → git diff → roll back → exit 0 (no commit, no push)
#
# Env vars:
#   INIT=v0.3.4        bootstrap from a specific version (when no v* tags exist)
#   ASSUME_YES=1       skip both menus (mode=release + final y/N); for CI wrapping
#
# Exit codes:
#   0  success (or chosen preview/preview-diff completed cleanly)
#   1  local-state error (dirty tree, wrong branch, jq missing, drift detected,
#      tag already exists, user aborted at the final y/N, etc.)
#   2  remote-state conflict (push rejected — developer must git pull --rebase or git fetch --tags)
#   3  bootstrap needed (no v* tags found; rerun with INIT=v0.1.0)
#
set -euo pipefail

# No positional args. All interaction is via the on-screen menu (and the
# INIT / ASSUME_YES env vars for non-interactive bootstrap / CI wrapping).
if [ -n "${1:-}" ]; then
  echo "::error::Unknown argument: $1" >&2
  echo "Usage: scripts/release.sh        # interactive menu" >&2
  echo "       ASSUME_YES=1 scripts/release.sh   # non-interactive release" >&2
  echo "       INIT=v0.3.4 scripts/release.sh   # first-time bootstrap" >&2
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

# ---- preview (text-only, no file changes yet) -------------------------------

print_preview() {
  cat <<EOF
── Release preview ──
next version:  ${VERSION}
next tag:      ${TAG}
files that WILL change (not yet written):
  package.json                              (${OLD_PKG} → ${VERSION})
  src-tauri/tauri.conf.json                 (${OLD_TAURI} → ${VERSION})
  src-tauri/Cargo.toml                      (${OLD_CARGO} → ${VERSION})
  release-please-manifest.json              ("src-tauri": "${OLD_MANIFEST}" → "${VERSION}")
commit message: ${COMMIT_MSG}
────────────────────
EOF
}

print_preview

# Sanity: refuse to push a tag that already exists on the remote.
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "::error::Tag ${TAG} already exists locally; refusing to push a duplicate." >&2
  echo "  If this is correct, run: git push origin ${TAG}" >&2
  echo "  If not, delete the local tag first: git tag -d ${TAG}" >&2
  exit 1
fi

# ---- mode selection -----------------------------------------------------------
#
# After every invocation the developer is asked to pick a mode. This is the
# single point of no return; the chosen branch then runs to completion
# without further interactive prompts (other than the y/N before commit+push,
# which ASSUME_YES=1 can skip).
#
# Modes:
#   1) preview       — no file writes, no commits, no pushes; exits 0
#   2) release       — rewrites 4 files, shows the actual git diff, prompts
#                       y/N, then commits + pushes main + pushes tag
#   3) preview-diff  — same as preview, but ALSO rewrites the 4 files
#                       temporarily, runs git diff to show the exact patch,
#                       then rolls back. Useful for verifying awk/jq rewrites
#                       against weird file content. No commit, no push.

MODE=""
if [ "${ASSUME_YES:-0}" = "1" ]; then
  MODE="release"
else
  while [ -z "$MODE" ]; do
    echo ""
    echo "Choose a mode:"
    echo "  1) preview        (no writes; show what would change)"
    echo "  2) release        (rewrite 4 files + commit + push main + push ${TAG})"
    echo "  3) preview-diff   (rewrite temporarily, show full git diff, then roll back)"
    printf "> "
    read -r choice
    case "$choice" in
      1|preview|"preview"|p|P)         MODE="preview" ;;
      2|release|r|R)                   MODE="release" ;;
      3|preview-diff|d|D)              MODE="preview-diff" ;;
      *)                               echo "Invalid choice: '$choice'. Try 1, 2, or 3." ;;
    esac
  done
fi

echo ""
echo "→ Mode: ${MODE}"

if [ "$MODE" = "preview" ]; then
  echo "(preview; no files written, no commits, no pushes)"
  echo "Re-run and pick 2 when ready to actually release."
  exit 0
fi

# ---- rewrite 4 files (shared by preview-diff and release) -------------------

rewrite_pkg
rewrite_tauri
rewrite_cargo
rewrite_manifest

# Show the actual git diff (this is what the bundle filenames will be
# produced from; verify visually before committing).
echo ""
echo "── git diff (4 files) ──"
git --no-pager diff -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
echo "────────────────────────"
echo ""

if [ "$MODE" = "preview-diff" ]; then
  echo "→ Rolling back the 4 files (preview-diff mode). No commit, no push."
if ! git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json 2>/dev/null; then

  echo '::warning::git checkout rollback failed (sandbox or unusual state). Restoring manually:'

  for f in package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json; do

    [ -f "$f" ] && git checkout HEAD -- "$f" 2>/dev/null || true

  done

fi
exit 0
fi

# ---- release path: confirm + commit + push ----------------------------------

# Confirm with the developer. Set ASSUME_YES=1 to skip (e.g. for CI wrapping).
if [ "${ASSUME_YES:-0}" != "1" ]; then
  read -r -p "Commit and push ${TAG}? [y/N] " ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *)
      echo "Aborted by user. Rolling back file edits."
if ! git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json 2>/dev/null; then

  echo '::warning::git checkout rollback failed (sandbox or unusual state). Restoring manually:'

  for f in package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json; do

    [ -f "$f" ] && git checkout HEAD -- "$f" 2>/dev/null || true

  done

fi
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
if ! git checkout -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json 2>/dev/null; then

  echo '::warning::git checkout rollback failed (sandbox or unusual state). Restoring manually:'

  for f in package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json; do

    [ -f "$f" ] && git checkout HEAD -- "$f" 2>/dev/null || true

  done

fi
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
