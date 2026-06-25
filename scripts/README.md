# `scripts/`

Local maintenance scripts for NiceSSH. These run on the developer's
machine, not in CI. CI handles the build-and-publish side (see
`.github/workflows/build-release.yml`); these scripts handle the
release-time bookkeeping that used to live in `.github/workflows/tag.yml`
before it was removed in favor of explicit, local control.

## Quick start

```sh
./manage.sh    # top-level menu
```

```
═══════════════════════════════════════
  NiceSSH 项目工具
═══════════════════════════════════════
  1) 打 tag 发版
  2) Tag 管理
  3) 修改版本号
  4) 查看版本号
  5) 退出
═══════════════════════════════════════
```

## What each option does

### 1) 打 tag 发版

The full release flow. Walks you through:
1. Reads the current `major.minor` from the 4 source files.
2. Computes `patch = git rev-list --count HEAD` (commit count on main).
3. Builds the preview: `next version = MAJOR.MINOR.{patch}`.
4. Checks that none of the 4 source files disagree (drift detection —
   catches the latent `tag.yml` bug where `package.json` was rewritten
   on disk but never `git add`-ed).
5. Tag-downgrade protection (D1): if the computed patch is **lower**
   than the last published tag's patch, refuses to publish (this
   happens after a rebase or force-push reduces the commit count).
6. Prints a preview block.
7. Prompts:
   - **`[Enter]`** — abort, no changes
   - **`d` + `[Enter]`** — apply the rewrites temporarily, show the
     full `git diff`, then roll back. Useful for visually verifying
     the awk/jq rewrites against odd file content.
   - **`y` + `[Enter]`** — commit + push `main` + push the new tag.
     GitHub Actions picks up the new tag (build-release.yml triggers
     on `push tags: ['v*']`) and runs the build/publish pipeline.

Override the major.minor with `MAJOR_MINOR=0.4 ./manage.sh`.

### 2) Tag 管理

Sub-menu:
- `1) 列本地 v* tag`
- `2) 删本地 v* tag`  — **requires `CONFIRM=1`**
- `3) 列远端 v* tag`
- `4) 删远端 v* tag`  — **requires `CONFIRM=1`**, 5-second countdown

Example:
```sh
CONFIRM=1 ./manage.sh    # then pick 2 → 2
```

The 5-second countdown is intentional: it gives you a chance to
Ctrl-C if you realize you fat-fingered the option. Local deletion
is recoverable via `git reflog`; remote deletion is not — that's
why both paths require an explicit `CONFIRM=1`.

### 3) 修改版本号

Set the 4 source files to a specific version. Three input formats:

- `X.Y.Z` (e.g. `0.3.5`, `1.0.0`, `2.1.0`) — literal
- `X.Y` (e.g. `0.4`, `1.0`) — patch is filled in as commit count
- `PATCH` — major.minor taken from current files, patch is commit count

This option is for **changing the major.minor line** (e.g. starting
`0.4.x`). Once you're on a new line, use option 1 to push and tag —
its `next version` calculation will use the new major.minor.

The script prompts `Commit? [y/N]` so you can review the diff first
by answering `n`.

### 4) 查看版本号

Read-only. Shows:
- The 4 source files' current version
- A drift warning if they disagree
- Git state (branch, HEAD, commit count)
- A **next-release preview** showing what option 1 would compute
  right now (`major.minor` from files + `patch` from commit count)

## Algorithms in use

### Version = `MAJOR.MINOR.PATCH`

`MAJOR.MINOR` is set explicitly by you (via option 3, or by hand,
or by passing `MAJOR_MINOR=0.4 ./manage.sh`).

`PATCH` is **always derived from `git rev-list --count HEAD`** —
the number of commits on the current branch. This is the algorithm
you asked for ("the last digit is the git commit count").

### Trade-off you accepted

Because patch = commit count, **rebasing or force-pushing can reduce
the commit count and produce a tag that's lower than an existing
one**. The script protects against this (D1):
- If `computed patch < last published patch` → refuse to publish
  with a clear error pointing at the rebase/force-push as the cause.
- If `computed tag == last published tag` → refuse; nothing new to
  release.

## Files in this directory

| File | Role |
|------|------|
| `../manage.sh` (at the project root) | Top-level menu. The one you run. Lives at the repo root so that `jq` / `awk` can find the 4 source files by relative path regardless of cwd. |
| `release.sh` | Thin wrapper around the manage.sh option-1 logic, kept for backward compatibility (and for any pre-existing CI hooks that reference it). Internally it `.`-sources `_version_files.sh` and does exactly what option 1 does. |
| `_version_files.sh` | Shared library. Provides the 4-file read/write functions, drift check, commit-count helper, rollback helper, and preflight checks. **Do not run directly.** `.`-source it. |
| `check-signer.sh` | (Unchanged.) Verify the Tauri updater signing key is loaded in the macOS keychain. |
| `merge-updater.mjs` | (Unchanged.) Node script that merges per-platform Tauri updater JSON into a single `latest.json` for the in-app updater. Runs inside the GitHub Actions release job. |

## Why this structure

The original `tag.yml` (now removed) combined three event sources
(`push main`, `workflow_run Test completed`, `workflow_dispatch`)
and produced occasional mismatches between the release title and
the bundle filenames (e.g. a v0.3.1 release containing
`NiceSSH_0.3.0_*.dmg`).

The current design:
- **Tag creation is initiated by you**, not by CI.
- The on-screen menu forces a conscious mode choice on every
  invocation (you can't accidentally type `--dry-run` thinking
  it ships; you can't accidentally ship without a preview).
- Tag-downgrade protection catches rebase/force-push regressions
  before they produce a bad release.
- Drift detection catches the latent `tag.yml` bug where
  `package.json` was rewritten on disk but never `git add`-ed.

See `docs/superpowers/specs/2026-06-25-release-script-design.md`
for the full design and rationale.
