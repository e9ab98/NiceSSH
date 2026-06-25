# `scripts/`

Local maintenance scripts for NiceSSH. These run on the developer's
machine, not in CI. CI handles the build-and-publish side (see
`.github/workflows/build-release.yml`); these scripts handle the
release-time bookkeeping that used to live in `.github/workflows/tag.yml`
before it was removed in favor of explicit, local control.

## `release.sh`

Cut a new NiceSSH release.

### One-line summary

```sh
scripts/release.sh     # interactive; you pick 1, 2, or 3 at the menu
```

That's the only way to call it — there are no CLI flags. The script
is the same on every invocation; the on-screen menu decides what
happens next.

### What it does

1. `git fetch --tags` to get the latest tag state.
2. Verifies the working tree is clean, you're on `main`, and `jq`
   is installed.
3. Derives the next version: parse the highest existing `vX.Y.Z`
   tag, bump the patch (`v0.3.42` → `v0.3.43`).
4. Reads the current version from each of the 4 source files
   (`package.json`, `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml`, `release-please-manifest.json`) and
   verifies they all agree. Exits 1 with a clear error if not
   (this is the canary that catches the latent `tag.yml` bug
   where `package.json` was rewritten on disk but never
   `git add`-ed).
5. Prints a text preview block (no file writes yet).
6. **Menu** — you pick one of three modes:

   ```
   Choose a mode:
     1) preview        (no writes; show what would change)
     2) release        (rewrite 4 files + commit + push main + push v0.3.43)
     3) preview-diff   (rewrite temporarily, show full git diff, then roll back)
   ```

   - **1 → preview**: exit 0. Nothing on disk, no commits, no
     pushes. The most cautious option.
   - **2 → release**: rewrites the 4 files, prints the actual
     `git diff`, prompts `Commit and push v0.3.43? [y/N]`, then
     commits + pushes `main` + pushes the `vX.Y.Z` tag. This is
     the one that actually ships.
   - **3 → preview-diff**: rewrites the 4 files, prints
     `git diff`, then rolls back via `git checkout --`. Useful
     for visually verifying the awk/jq rewrites when the
     source file shape is unusual.

7. On the release path, after the y/N confirm, the script does:
   - `git add` the four files
   - `git commit -m "chore(release): v${VERSION}"` (or exits 1 if
     there's nothing to commit — i.e. you ran the script twice
     and the second run found no diff to apply)
   - `git push origin HEAD:main` (exits 2 on rejection, with a
     `git pull --rebase` hint)
   - `git push origin refs/tags/${TAG}` (exits 2 on rejection)

8. GitHub Actions picks up the new tag (the `Build & Publish
   Release` workflow triggers on `push tags: ['v*']`) and runs
   the build/publish pipeline (`.github/workflows/build-release.yml`).

### Example session

```
$ scripts/release.sh
── Release preview ──
next version:  0.3.43
next tag:      v0.3.43
files that WILL change (not yet written):
  package.json                              (0.3.42 → 0.3.43)
  src-tauri/tauri.conf.json                 (0.3.42 → 0.3.43)
  src-tauri/Cargo.toml                      (0.3.42 → 0.3.43)
  release-please-manifest.json              ("src-tauri": "0.3.42" → "0.3.43")
commit message: chore(release): v0.3.43
────────────────────

Choose a mode:
  1) preview        (no writes; show what would change)
  2) release        (rewrite 4 files + commit + push main + push v0.3.43)
  3) preview-diff   (rewrite temporarily, show full git diff, then roll back)
> 1
→ Mode: preview
(preview; no writes, no commits, no pushes)
Re-run and pick 2 when ready to actually release.
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success — release pushed, or `preview`/`preview-diff` completed cleanly |
| 1 | Local-state error: dirty tree, wrong branch, `jq` missing, drift detected, tag already exists locally, user aborted at the final y/N |
| 2 | Remote-state conflict — `git push` was rejected. Resolve manually (see Troubleshooting). |
| 3 | Bootstrap needed — no `v*` tags found. Rerun with `INIT=v0.3.4 scripts/release.sh`. |

### Before you release

- [ ] `pnpm test` (or `npm test`) is green locally
- [ ] Working tree is clean (`git status` is empty)
- [ ] On `main` (`git branch --show-current` says `main`)
- [ ] Last published `v*` tag is what you expect
      (`git tag -l "v*" --sort=-v:refname | head -1`)

### Troubleshooting

**"Working tree is not clean"** — commit or stash your changes
first. Untracked files also count as dirty.

**"Must be on 'main'"** — `git checkout main`. The script
intentionally refuses to tag from a feature branch.

**"No v* tags found locally"** (exit 3) — this repo has never
been tagged. Bootstrap with the version you want to start from:

```sh
git tag v0.3.4                                            # create the bootstrap tag locally
INIT=v0.3.4 scripts/release.sh                            # then pick 2 at the menu
```

**"Version drift detected"** (exit 1) — the four files don't
all agree on the current version. This usually means a prior
`release.sh` run failed mid-flight and a partial edit was left
behind, or the old CI workflow left `package.json` out of
sync (it had a known bug of rewriting `package.json` on disk
but forgetting to `git add` it). Fix manually by editing the
listed files to match, then re-run.

**"git push origin main was rejected" (exit 2)** — someone
else pushed a commit to `main` after your last `git pull`. The
script will not auto-rebase. Resolve manually:

```sh
git pull --rebase           # or: git fetch + git rebase origin/main
scripts/release.sh          # re-run; the script will re-pick the version
```

If you've already pushed the version commit (e.g. you ran
`git push origin main` by hand), you can just push the tag:

```sh
git push origin v0.3.43
```

**"Tag v0.3.X already exists locally"** (exit 1) — a previous
run created the tag but failed before pushing it. Either push
it (`git push origin v0.3.X`) or delete it
(`git tag -d v0.3.X`) and re-run.

### Non-interactive mode

For CI wrappers (e.g. a pre-push hook that wants to invoke this
script), set `ASSUME_YES=1` to skip the menu **and** the
y/N confirm:

```sh
ASSUME_YES=1 scripts/release.sh
```

This forces `mode=release` and proceeds straight to commit +
push. The script still exits 1 / 2 / 3 on errors; `ASSUME_YES`
only suppresses the interactive prompts.

### Why a local script (not CI)?

The release pipeline used to tag itself via
`.github/workflows/tag.yml` (now removed). That approach
combined three event sources (`push main`, `workflow_run Test
completed`, `workflow_dispatch`) and produced occasional
mismatches between the release title and the bundle filenames
(e.g. a v0.3.1 release containing `NiceSSH_0.3.0_*.dmg`).
Moving the tag creation to a single explicit
`scripts/release.sh` run removes the timing race entirely, and
the on-screen menu forces a human to consciously choose
between preview and release on every invocation. See
`docs/superpowers/specs/2026-06-25-release-script-design.md`
for the full design and rationale.

## Other scripts

- `check-signer.sh` — verify the Tauri updater signing key is
  loaded in the macOS keychain.
- `merge-updater.mjs` — Node script that merges per-platform
  Tauri updater JSON into a single `latest.json` for the
  in-app updater. Runs inside the GitHub Actions release job.
