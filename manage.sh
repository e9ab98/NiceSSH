#!/usr/bin/env bash
#
# manage.sh — top-level project tool for NiceSSH.
#
# Combines release (option 1), tag management (option 2), set-version (option 3),
# and view-version (option 4) into a single interactive entry point.
#
# Lives in the project root so it can find package.json, Cargo.toml, etc. by
# relative path regardless of cwd. The shared library
# scripts/_version_files.sh is sourced relative to this script's own location.

set -euo pipefail

# Always run from the repo root, so jq/awk find the 4 source files by relative path.
if ! REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"; then
  echo "::error::manage.sh must be run from inside a git repository (or with a path to a repo script)." >&2
  echo "  Current dir: $(pwd)" >&2
  echo "  Try: cd /path/to/NiceSSH && bash manage.sh" >&2
  exit 1
fi
cd "$REPO_ROOT"

# Source the shared library relative to the repo root (works regardless of how
# the script was invoked: ./manage.sh, bash manage.sh, bash /path/to/manage.sh).
# shellcheck source=scripts/_version_files.sh
. "${REPO_ROOT}/scripts/_version_files.sh"

# ---- top-level menu ---------------------------------------------------------

print_menu() {
  cat <<'MENU'

═══════════════════════════════════════
  NiceSSH 项目工具
═══════════════════════════════════════
  1) 打 tag 发版    (改 4 文件到 next version + commit + push main + push tag)
  2) Tag 管理       (列本地 / 删本地 / 列远端 / 删远端 — 删必须 --confirm)
  3) 修改版本号     (改 4 文件到 X.Y.Z 或 PATCH; 不 commit, 你自己提交)
  4) 查看版本号     (只读打印 4 文件当前 version + 下一个 next 预览)
  5) 退出
═══════════════════════════════════════
MENU
}

# ---- option 1: release ------------------------------------------------------

opt_release() {
  echo ""
  echo "── Option 1: 打 tag 发版 ──"
  echo ""

  # Pre-flight checks
  require_jq || return 1
  require_main_branch || return 1
  require_clean_tree || return 1

  git fetch --tags --force >/dev/null

  # Derive next version
  local major_minor patch version tag last_tag last_patch
  major_minor="${MAJOR_MINOR:-$(get_major_minor)}"
  if [ -z "$major_minor" ] || ! [[ "$major_minor" =~ ^[0-9]+\.[0-9]+$ ]]; then
    echo "::error::Cannot determine major.minor from current files ('${major_minor}')." >&2
    echo "  Use option 3 first to set a version, or pass: MAJOR_MINOR=0.3 $0" >&2
    return 1
  fi
  patch="$(commit_count)"
  version="${major_minor}.${patch}"
  tag="v${version}"
  local commit_msg="chore(release): v${version}"

  # D1: tag-downgrade protection
  last_tag="$(git tag -l "v${major_minor}.*" --sort=-v:refname | head -1 || true)"
  if [ -n "$last_tag" ] && [[ "$last_tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    last_patch="${BASH_REMATCH[3]}"
    if [ "$patch" -lt "$last_patch" ]; then
      echo "::error::Computed patch (${patch}) < last published patch (${last_patch})." >&2
      echo "  Likely cause: rebase/force-push reduced commit count." >&2
      echo "  Fix: rebase on top of the latest tag, or pass MAJOR_MINOR=<new.line>." >&2
      return 1
    fi
    if [ "$last_tag" = "$tag" ]; then
      echo "::error::Computed tag ${tag} equals last published tag. Make a new commit first." >&2
      return 1
    fi
  fi

  # Drift check
  check_drift || return 1

  # Read current version
  local old_pkg old_tauri old_cargo old_manifest
  old_pkg="$(read_pkg_version)"
  old_tauri="$(read_tauri_version)"
  old_cargo="$(read_cargo_version)"
  old_manifest="$(read_manifest_version)"

  # E1: flat menu — preview then Enter/d/q
  cat <<EOF
── Release preview ──
major.minor:    ${major_minor}    (from current source files)
commit count:   ${patch}    (git rev-list --count HEAD)
next version:   ${version}
next tag:       ${tag}
files that WILL change:
  package.json                              (${old_pkg} → ${version})
  src-tauri/tauri.conf.json                 (${old_tauri} → ${version})
  src-tauri/Cargo.toml                      (${old_cargo} → ${version})
  release-please-manifest.json              ("src-tauri": "${old_manifest}" → "${version}")
commit message: ${commit_msg}
────────────────────
EOF

  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "::error::Tag ${tag} already exists locally; refusing to push a duplicate." >&2
    return 1
  fi

  echo ""
  printf "  [Enter] abort    d + [Enter] show full diff    y + [Enter] commit + push\n> "
  read -r reply

  case "$reply" in
    y|Y|yes|release)
      : # fall through
      ;;
    d|D|diff)
      rewrite_4_files "$version"
      echo ""
      echo "── git diff (4 files) ──"
      git --no-pager diff -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
      echo "────────────────────────"
      rollback_4_files
      echo "(rolled back; re-run option 1 to actually commit + push)"
      return 0
      ;;
    *)
      echo "Aborted; no changes made."
      return 1
      ;;
  esac

  # Release path
  trap 'echo "::error::Release failed mid-flight; rolling back." >&2; rollback_4_files' ERR
  rewrite_4_files "$version"
  trap - ERR

  git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml release-please-manifest.json
  if git diff --cached --quiet; then
    echo "::error::No changes staged; aborting." >&2
    rollback_4_files
    return 1
  fi

  git commit -m "$commit_msg"

  echo "→ Pushing main…"
  if ! git push origin HEAD:main; then
    echo "::error::git push origin main was rejected. Resolve with: git pull --rebase" >&2
    return 2
  fi

  echo "→ Creating tag ${tag}…"
  if ! git tag -a "${tag}" -m "Release ${tag}"; then
    echo "::error::git tag -a ${tag} failed. Resolve manually (e.g. delete a conflicting tag), then re-run." >&2
    return 1
  fi

  echo "→ Pushing tag ${tag}…"
  if ! git push origin "refs/tags/${tag}"; then
    echo "::error::git push origin ${tag} was rejected. Local tag exists. Retry: git push origin ${tag}" >&2
    return 2
  fi

  echo ""
  echo "✅ Released ${tag}"
  echo "→ Build & Publish Release will run on the new tag."
}

# ---- option 2: tag management -----------------------------------------------

opt_tags() {
  echo ""
  echo "── Option 2: Tag 管理 ──"
  echo ""
  echo "  1) 列本地 v* tag"
  echo "  2) 删本地 v* tag     (需要 --confirm)"
  echo "  3) 列远端 v* tag"
  echo "  4) 删远端 v* tag     (需要 --confirm)"
  echo "  5) 返回主菜单"
  printf "> "
  read -r sub

  case "$sub" in
    1) list_local_tags ;;
    2) delete_local_tags ;;
    3) list_remote_tags ;;
    4) delete_remote_tags ;;
    5|"") return 0 ;;
    *) echo "Unknown choice: '${sub}'"; return 1 ;;
  esac
}

list_local_tags() {
  echo ""
  echo "── Local v* tags ──"
  git tag -l "v*" --sort=-v:refname
  if [ -z "$(git tag -l "v*")" ]; then
    echo "  (none)"
  fi
}

delete_local_tags() {
  local tags
  tags="$(git tag -l "v*")"
  if [ -z "$tags" ]; then
    echo "No local v* tags to delete."
    return 0
  fi
  echo ""
  echo "Would delete these LOCAL tags:"
  echo "$tags" | sed 's/^/  /'
  echo ""
  if [ "${CONFIRM:-0}" != "1" ]; then
    echo "Refusing to delete. Re-run with: CONFIRM=1 $0   (or pick option 2 again after exporting)"
    return 1
  fi
  echo "Deleting in 5 seconds — press Ctrl-C to abort…"
  sleep 5
  echo "$tags" | xargs -n1 git tag -d
  echo "Done."
}

list_remote_tags() {
  echo ""
  echo "── Remote v* tags (origin) ──"
  git ls-remote --tags origin 2>/dev/null | sed -n 's/.*refs\/tags\///p' | grep '^v' | sort -V -r
  if [ -z "$(git ls-remote --tags origin 2>/dev/null | grep -E 'refs/tags/v')" ]; then
    echo "  (none or no network)"
  fi
}

delete_remote_tags() {
  echo ""
  echo "── Fetching remote tag list first…"
  git fetch --tags --force >/dev/null 2>&1 || true
  local remote_tags
  remote_tags="$(git ls-remote --tags origin 2>/dev/null | sed -n 's/.*refs\/tags\///p' | grep '^v' || true)"
  if [ -z "$remote_tags" ]; then
    echo "No remote v* tags to delete (or no network)."
    return 0
  fi
  echo "Would delete these REMOTE tags from 'origin':"
  echo "$remote_tags" | sed 's/^/  /'
  echo ""
  if [ "${CONFIRM:-0}" != "1" ]; then
    echo "Refusing to delete. Re-run with: CONFIRM=1 $0"
    return 1
  fi
  echo "Deleting REMOTE tags in 5 seconds — press Ctrl-C to abort…"
  sleep 5
  for t in $remote_tags; do
    git push origin ":refs/tags/${t}" || echo "  failed to delete ${t}"
  done
  echo "Done."
}

# ---- option 3: set-version --------------------------------------------------

opt_set_version() {
  echo ""
  echo "── Option 3: 修改版本号 ──"
  echo ""
  echo "  This rewrites the 4 source files to your target version."
  echo "  It does NOT commit, push, or tag — you handle that yourself."
  echo ""
  echo "  Formats:"
  echo "    X.Y.Z   literal (e.g. 0.3.5, 1.0.0, 2.1.0)"
  echo "    PATCH   use current major.minor from the 4 files; patch = commit count"
  echo ""
  printf "Target version (X.Y.Z / PATCH): "
  read -r input

  local version
  case "$input" in
    "")
      echo "Aborted."
      return 1
      ;;
    PATCH|patch|Patch)
      local major_minor patch
      major_minor="$(get_major_minor)"
      patch="$(commit_count)"
      version="${major_minor}.${patch}"
      ;;
    *)
      if [[ "$input" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        version="$input"
      else
        echo "::error::Invalid version format: '${input}'. Use X.Y.Z or PATCH." >&2
        return 1
      fi
      ;;
  esac

  # Pre-flight (no clean-tree check — the user can run this on a dirty tree
  # if they're just trying out a value, since we don't commit anyway).
  require_jq || return 1

  local old_pkg old_tauri old_cargo old_manifest
  old_pkg="$(read_pkg_version)"
  old_tauri="$(read_tauri_version)"
  old_cargo="$(read_cargo_version)"
  old_manifest="$(read_manifest_version)"

  echo ""
  echo "Will rewrite:"
  echo "  package.json                              (${old_pkg} → ${version})"
  echo "  src-tauri/tauri.conf.json                 (${old_tauri} → ${version})"
  echo "  src-tauri/Cargo.toml                      (${old_cargo} → ${version})"
  echo "  release-please-manifest.json              (${old_manifest} → ${version})"
  echo ""
  printf "Apply? [y/N] "
  read -r ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *) echo "Aborted; no changes made."; return 1 ;;
  esac

  # Roll back on any mid-flight failure.
  trap 'echo "::error::set-version failed mid-flight; rolling back." >&2; rollback_4_files' ERR
  rewrite_4_files "$version"
  trap - ERR

  echo ""
  echo "✅ Rewrote 4 files to ${version} (not committed; review with 'git diff' then commit yourself)."
}

# ---- option 4: view-version -------------------------------------------------

opt_view_version() {
  echo ""
  echo "── Option 4: 查看版本号 ──"
  echo ""
  local pkg tauri cargo manifest
  pkg="$(read_pkg_version)"
  tauri="$(read_tauri_version)"
  cargo="$(read_cargo_version)"
  manifest="$(read_manifest_version)"

  echo "Current source-file versions:"
  echo "  package.json:                ${pkg}"
  echo "  src-tauri/tauri.conf.json:   ${tauri}"
  echo "  src-tauri/Cargo.toml:        ${cargo}"
  echo "  release-please-manifest.json: ${manifest}"

  if [ "$pkg" != "$tauri" ] || [ "$tauri" != "$cargo" ] || [ "$tauri" != "$manifest" ]; then
    echo ""
    echo "  ⚠️  Drift detected — files disagree. Use option 3 to fix."
  fi

  echo ""
  echo "Git state:"
  echo "  current branch:  $(git rev-parse --abbrev-ref HEAD)"
  echo "  HEAD commit:     $(git rev-parse --short HEAD)"
  echo "  commit count:    $(commit_count)"

  # Next-version preview (F2)
  local major_minor patch next_version last_tag
  major_minor="${pkg%.*}"
  patch="$(commit_count)"
  next_version="${major_minor}.${patch}"
  last_tag="$(git tag -l "v*" --sort=-v:refname | head -1)"
  echo ""
  echo "Next release preview (if you ran option 1 now):"
  echo "  major.minor:    ${major_minor}    (from current source files)"
  echo "  patch:          ${patch}    (commit count)"
  echo "  next version:   ${next_version}"
  if [ -n "$last_tag" ]; then
    echo "  last tag:       ${last_tag}"
  else
    echo "  last tag:       (none — first release)"
  fi
}

# ---- main loop --------------------------------------------------------------

while true; do
  print_menu
  printf "> "
  read -r choice
  case "$choice" in
    1) opt_release ;;
    2) opt_tags ;;
    3) opt_set_version ;;
    4) opt_view_version ;;
    5|q|Q|quit|exit) echo "Bye."; exit 0 ;;
    "") echo "(no choice — try 1, 2, 3, 4, or 5)" ;;
    *) echo "Unknown choice: '${choice}'" ;;
  esac
done
