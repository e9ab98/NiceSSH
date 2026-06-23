# Changelog

All notable changes to NiceSSH are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- [CI] Auto-install Linux build dependencies in rust-checks
- [Rust] Sanitize test directory suffix to support Windows file paths
- [Rust] Prefix unused cross-platform variables with underscores for Windows build
- [CI] Run macOS Intel build on Apple Silicon runner for faster queue times
- [CI] Handle HTTP 422 error when deleting nonexistent latest tag
- [UI] Set application icon from logo
- [Rust] Force `ssh-add` to use the SSH_ASKPASS path so the GUI's
  PassphraseDialog is not hijacked by an inherited controlling TTY
  (fixes identity-switch hang in `cargo tauri dev` and certain launch
  contexts). New module `src-tauri/src/commands/ssh_add_askpass.rs`
  uses `Command::spawn` with `Command::pre_exec(setsid)` to detach
  the child from the parent's controlling terminal, combined with
  `SSH_ASKPASS_REQUIRE=force`, a closed stdin, and a 30s wall-clock
  timeout. We tried a manual `fork+setsid+execve` implementation
  first (which guarantees setsid runs), but it duplicated the
  parent's environment and broke `SSH_AUTH_SOCK` inheritance from
  the launchd-managed agent; rolling back to `Command::spawn`
  restores the standard env-inheritance path.

- [Update] In-app auto-update flow (Tauri updater plugin, signature-verified). On startup (≤24h cache, opt-out toggle in Settings), the app checks the GitHub release endpoint and shows a one-time toast per new version. Settings → Updates has a new tab with current/latest version, "Check now", "Update now" (download + progress + manual close-and-reopen to apply), and a notify toggle. Release pipeline (`release.yml`) requires `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets at publish time; the secret check is the first step of the release job, so unsigned releases fail closed (AC6). Pre-1.0.0 caveat: macOS Gatekeeper and Windows SmartScreen still prompt on first open of an auto-updated build (same as a manual download; notarization is v1.0.0 work). Linux `.deb`/`.rpm` updates require root; AppImage is the recommended auto-update channel. Implementation: G1–G5 / AC1–AC12 / R-1..R-8 in `docs/superpowers/specs/2026-06-18-v030-auto-update-design.md`. **First-release note**: the per-binary signing + `latest.json` generation in CI lands in v0.3.1, so this v0.3.0 binary does not yet receive in-app update prompts on subsequent runs. v0.3.0 → v0.3.1 is the first e2e in-app update path; the framework (code, tests, Settings UI, CI secret check) is all in place and exercised locally.
### Planned
- v0.2.0 — Settings polish, log viewer UI, keyboard shortcuts

- v0.3.x — SSH config editor (visual editor for Host blocks, Match, Include — baseline add/edit first, then rich directives)
- v1.0.0 — macOS notarization, Windows code signing, performance polish

---

## [0.1.0] — 2026-06-17

Initial public release. MVP feature set with 4 core flows working end-to-end.

### Added
- **SSH key management**
  - List all keys in `~/.ssh/` with type, fingerprint, comment
  - Generate new keys (ED25519 or RSA 4096) with optional passphrase
  - Delete keys (with confirmation)
  - Copy public key to clipboard
  - View full public key
- **Git identity management**
  - Create / edit / delete identities (label, name, email, key path, match path, host alias, git host)
  - Directory-based auto-matching via `~/.gitconfig`'s `includeIf` strategy
  - Per-identity `~/.gitconfig-<label>` subfile with `[user]` and `[core] sshCommand`
- **Project management**
  - Add local git repos by browsing or path entry
  - Bind projects to identities (per-project override of `includeIf`)
  - Show recent 10 commits (read-only) for each project
  - "Test SSH" button: runs `ssh -T git@<host>` and shows the authenticated username
  - Switch identity on a project from the detail view
- **SSH config viewer**
  - Parse `~/.ssh/config` into Host blocks (including Match blocks)
  - Display directives per block, distinguish managed (App-created) vs user-written
  - "Validate" button runs `sshd -T` to check the file
- **Modification history**
  - Every write to `~/.ssh/config`, `~/.gitconfig`, `~/.gitconfig-<label>`, and `~/.nicessh/config.json` is snapshotted
  - Last 50 snapshots retained (oldest auto-trimmed)
  - Revert any past snapshot with one click
  - Auto-snapshot of current state before each revert (chain-safe)
- **UI**
  - Sourcetree-style two-pane layout (left project tree, right state card)
  - Sidebar with 6 routes (Projects / Identities / SSH Keys / SSH Config / History / Settings)
  - Follows system dark / light theme with no flash of unstyled content
  - framer-motion indicator animation in the sidebar
  - shadcn/ui-style components (Button, Card, Input, Label, Badge, Dialog)
  - Toast notifications for errors (sonner)
- **Cross-platform**
  - macOS (Intel + Apple Silicon, universal2 .dmg + per-arch .dmg)
  - Windows (x64 .msi)
  - Linux (x64 .AppImage + .deb)
  - GitHub Actions release pipeline: 7+ artifacts per release tag
- **Safety**
  - Atomic file writes (tmp + rename) — no half-written configs on crash
  - App-managed SSH config blocks marked with `# nicessh-managed`; user-written blocks preserved byte-for-byte
  - Per-operation history snapshot before every change
  - 30s timeout on all subprocess calls (ssh, git, ssh-keygen, ssh-add)
  - 4KB output truncation on subprocess output
  - `~/.ssh/` permissions enforced to 0700 on Unix
  - Passphrases never written to disk; held in React state only

### Technical
- Built with Tauri 2.11 + Rust 1.78+ + React 18 + TypeScript 5 + Vite 5
- 9 Rust modules: `error`, `paths`, `fs_safety`, `history`, `config_store`, `ssh_config`, `git_config`, `ssh_keys`, `runner`
- 23 `#[tauri::command]` IPC handlers
- 30+ Rust unit tests covering parser, history rollback, atomic write, path expansion
- Frontend: Zustand state, react-router-dom v6 routing, lucide-react icons
- Subprocess execution with timeout and output truncation

### Known limitations
- **No code signing / notarization.** macOS Gatekeeper will warn on first open; Windows SmartScreen will block the .msi. Workaround documented in README.
- **No in-app terminal.** `git push` etc. still happens in your terminal.
- **No commit / push / branch UI.** The app shows the last 10 commits read-only; actual Git write operations are out of scope.
- **English only.** i18n is a v1.x addition.
- **No passphrase unlock flow on key generation.** If you set a passphrase when generating, `ssh-add` integration for later unlocks is in v0.2.

### Security
- This is the first public release. The app reads and writes files in `~/.ssh/`, `~/.gitconfig`, and `~/.nicessh/`. It does not transmit any data over the network. The only network calls are the ones you explicitly trigger (the `ssh -T git@github.com` test).
- If you find a security issue, please email security@<your-domain> (do not file a public GitHub issue).

---

## Release process

This project uses [Semantic Versioning](https://semver.org/):
- **MAJOR** — incompatible API / data format changes
- **MINOR** — new features, backward-compatible
- **PATCH** — bug fixes, backward-compatible

Releases are automated via `.github/workflows/release.yml` on `v*.*.*` git tags.
The full release pipeline builds 7+ artifacts and publishes a GitHub Release.

Version bumps and CHANGELOG updates are automated via
[release-please](https://github.com/googleapis/release-please) (see
`.github/workflows/release-please.yml` + `release-please-config.json`).
Conventional commit messages on `main` (e.g. `feat:`, `fix:`,
`feat!:` for breaking changes) drive the next version number; the
Release PR is reviewed and merged, and release-please pushes the
tag automatically, which triggers `release.yml`.

v0.3.0 is the last release shipped with the manual bump + tag flow.
