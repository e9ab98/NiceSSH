#!/usr/bin/env bash
#
# scripts/release.sh — cut a new NiceSSH release from your local machine.
#
# This is now a thin wrapper around the shared lib. The user-facing flow
# (preview / release / preview-diff) is preserved for backward compatibility,
# but new code should call scripts/manage.sh and pick option 1.
#
# Behavior (see docs/superpowers/specs/2026-06-25-release-script-design.md):
#   1. fetch --tags from origin
#   2. verify working tree clean, on main, jq present
#   3. derive next version = current major.minor + commit count on HEAD
#   4. check 4 source files for version drift (exit 1 if they disagree)
#   5. print a preview block
#   6. prompt:  [Enter] commit+push  /  d [Enter] show diff  /  q [Enter] abort
#      (ASSUME_YES=1 forces release without prompts)
#
# Env vars:
#   MAJOR_MINOR="X.Y"   override the major.minor (default: read from current
#                       version in the 4 source files)
#   ASSUME_YES=1        skip the Enter/d/q prompt; go straight to commit+push
#
# Exit codes:
#   0  success — release pushed
#   1  local-state error: dirty tree, wrong branch, jq missing, drift, user abort
#   2  remote-state conflict — git push was rejected
#   3  no v* tags and no MAJOR_MINOR override (bootstrap)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/_version_files.sh
. "$SCRIPT_DIR/_version_files.sh"

# ---- preflight --------------------------------------------------------------

require_jq
require_main_branch
require_clean_tree

git fetch --tags --force >/dev/null

# ---- derive next version ----------------------------------------------------

MAJOR_MINOR="${MAJOR_MINOR:-$(get_major_minor)}"
if [ -z "$MAJOR_MINOR" ] || ! [[ "$MAJOR_MINOR" =~ ^[0-9]+\.[0-9]+$ ]]; then
  echo "::error::MAJOR_MINOR ('${MAJOR_MINOR}') is empty or malformed." >&2
  echo "  Pass it explicitly: MAJOR_MINOR=0.3 scripts/release.sh" >&2
  echo "  Or first run 'scripts/manage.sh' option 3 to set a version on the 4 files." >&2
  exit 3
fi

PATCH="$(commit_count)"
VERSION="${MAJOR_MINOR}.${PATCH}"
TAG="v${VERSION}"
COMMIT_MSG="chore(release): v${VERSION}"

# ---- D1: tag-downgrade protection -------------------------------------------
#
# If commit count is smaller than the last published tag's patch (e.g. after
# a rebase), the computed next version would be a downgrade. Refuse rather
# than fail at git push.
LAST_TAG="$(git tag -l "v${MAJOR_MINOR}.*" --sort=-v:refname | head -1 || true)"
if [ -n "$LAST_TAG" ] && [[ "$LAST_TAG" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  LAST_PATCH="${BASH_REMATCH[3]}"
  if [ "$PATCH" -lt "$LAST_PATCH" ]; then
    echo "::error::Computed patch (${PATCH}) is LOWER than the last published tag's patch (${LAST_PATCH})." >&2
    echo "  This usually means a rebase or force-push reduced the commit count." >&2
    echo "  Refusing to publish a downgrade." >&2
    echo "" >&2
    echo "  Options:" >&2
    echo "    1) Drop the rebased commits and start from the last published tag" >&2
    echo "    2) Bump MAJOR.MINOR to a new line: MAJOR_MINOR=0.4 scripts/release.sh" >&2
    echo "    3) Manually delete the conflicting tag(s) and re-run" >&2
    exit 1
  fi
  if [ "$LAST_TAG" = "$TAG" ]; then
    echo "::error::Computed tag ${TAG} equals the last published tag. Nothing new to release." >&2
    echo "  Make a new commit first, or pass MAJOR_MINOR=<new.line>." >&2
    exit 1
  fi
fi

# ---- drift check ------------------------------------------------------------

check_drift

# ---- read current version for the preview ----------------------------------

OLD_PKG="$(read_pkg_version)"
OLD_TAURI="$(read_tauri_version)"
OLD_CARGO="$(read_cargo_version)"
OLD_MANIFEST="$(read_manifest_version)"

# ---- preview (E1: flat menu — preview then release/abort) ------------------

print_preview() {
  cat <<EOF
── Release preview ──
major.minor:    ${MAJOR_MINOR}    (from current source files; override with MAJOR_MINOR env var)
commit count:   ${PATCH}    (git rev-list --count HEAD)
next version:   ${VERSION}
next tag:       ${TAG}
files that WILL change:
  package.json                              (${OLD_PKG} → ${VERSION})
  src-tauri/tauri.conf.json                 (${OLD_TAURI} → ${VERSION})
  src-tauri/Cargo.toml                      (${OLD_CARGO} → ${VERSION})
  release-please-manifest.json              ("src-tauri": "${OLD_MANIFEST}" → "${VERSION}")
commit message: ${COMMIT_MSG}
────────────────────
EOF
}

print_preview

# Sanity: refuse to push a tag that already exists locally.
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "::error::Tag ${TAG} already exists locally; refusing to push a duplicate." >&2
  echo "  Push manually: git push origin ${TAG}" >&2
  echo "  Or delete the local tag first: git tag -d ${TAG}" >&2
  exit 1
fi

if [ "${ASSUME_YES:-0}" = "1" ]; then
  REPLY="y"
else
  echo ""
  printf "  [Enter] commit + push main + push ${TAG}    d + [Enter] show full diff    q + [Enter] abort\n> "
  read -r REPLY
fi

case "$REPLY" in
  q|Q|quit|abort|"")
    echo "Aborted; no changes made."
    exit 1
    ;;
  d|D|diff)
    # Apply the rewrites temporarily to show the diff, then roll back.
    rewrite_4_files "$VERSION"
    echo ""
    echo "── git diff (4 files) ──"
    git --no-pager diff -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
    echo "────────────────────────"
    echo ""
    rollback_4_files
    echo "(rolled back; re-run scripts/release.sh to actually commit + push)"
    exit 0
    ;;
  y|Y|yes|release)
    : # fall through to commit+push
    ;;
  *)
    echo "Unknown choice: '${REPLY}'. Use Enter (abort), d (diff), or y (release)."
    exit 1
    ;;
esac

# ---- release path: rewrite + commit + push ---------------------------------

# Roll back on any mid-flight failure.
trap 'echo "::error::Release failed mid-flight; rolling back file edits." >&2; rollback_4_files' ERR

rewrite_4_files "$VERSION"

# From here on we don't want the rollback trap — the commit is the next step.
trap - ERR

git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json

# Refuse to push a no-op (e.g. user ran the script twice in a row).
if git diff --cached --quiet; then
  echo "::error::No changes staged for commit; aborting before push." >&2
  rollback_4_files
  exit 1
fi

git commit -m "$COMMIT_MSG"

# Push main.
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

# Push the tag.
echo "→ Pushing tag ${TAG}…"
if ! git push origin "refs/tags/${TAG}"; then
  echo "::error::git push origin ${TAG} was rejected." >&2
  echo "  Local tag is created but NOT pushed. To retry: git push origin ${TAG}" >&2
  exit 2
fi

echo ""
echo "✅ Released ${TAG}"
echo "→ Build & Publish Release will run on the new tag."
echo "→ Watch progress: https://github.com/e9ab98/NiceSSH/actions"
