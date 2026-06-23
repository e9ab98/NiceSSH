# NiceSSH

[English](README.md) · [简体中文](README.zh-CN.md)

> A cross-platform desktop GUI for managing multiple SSH keys, Git identities, and project-to-account bindings.
> Stop editing `~/.ssh/config` and `~/.gitconfig` by hand. Stop pushing to the wrong GitHub account.

[![Release](https://img.shields.io/github/v/release/e9ab98/NiceSSH)](https://github.com/e9ab98/NiceSSH/releases)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue)](#download)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

## Features

- 🔑 **SSH key management** — list, generate (ED25519 / RSA 4096), delete, copy public key
- 👤 **Git identity management** — name, email, key, directory match (`includeIf`)
- 📁 **Project management** — register local repos, bind them to identities
- 🪄 **Directory auto-matching** — `~/work/` repos automatically use your work identity, zero-friction
- 🔍 **Visual SSH config editor** — your hand-written blocks are preserved byte-for-byte
- 🧪 **SSH connection test** — verify which GitHub account a push will hit
- ↩️ **Modification history with one-click rollback** — every change to `~/.ssh/config`, `~/.gitconfig`, and the app config is snapshotted
- 🌗 **Follows system dark / light theme**
- 🌐 **i18n** — English and 简体中文 built in, switch from Settings → Language
- 🖥️ **Native binaries** — Tauri 2.x, ~5–15 MB, runs as a real native app

## Why

If you've ever:

- Accidentally pushed to your work GitHub with your personal email
- Spent 30 minutes hand-editing `~/.ssh/config` to add a new account
- Lost track of which SSH key is for which GitHub
- Broken your `~/.gitconfig` and lost all your `includeIf` rules
- Wanted a "preview before commit" view of what your SSH config will look like

NiceSSH is for you.

## Download

Grab the latest release for your platform from the [**Releases page**](https://github.com/e9ab98/NiceSSH/releases/latest).

| Platform | Architecture | File |
|---|---|---|
| **macOS** (recommended) | Intel + Apple Silicon (universal2) | `NiceSSH-v*-macOS-universal.dmg` |
| macOS Intel only | x86_64 | `NiceSSH-v*-darwin-x64.dmg` |
| macOS Apple Silicon only | aarch64 | `NiceSSH-v*-darwin-arm64.dmg` |
| Windows | x86_64 | `NiceSSH-v*-windows-x64.msi` |
| Linux | x86_64 | `NiceSSH-v*-linux-x64.AppImage` or `.deb` |

> **Tip:** A rolling "Latest" build (every commit to `main`) is always available at <https://github.com/e9ab98/NiceSSH/releases/tag/latest>. Use it for testing; use versioned releases for stability.

### Install

**macOS**
1. Open the `.dmg`
2. Drag `NiceSSH.app` to `/Applications`
3. First open: right-click → Open (bypasses Gatekeeper if unsigned; pre-1.0)

**Windows**
1. Run the `.msi` installer
2. SmartScreen may show a warning for unsigned builds (pre-1.0) — click "More info" → "Run anyway"

**Linux (AppImage)**
```bash
chmod +x NiceSSH-v*-linux-x64.AppImage
./NiceSSH-v*-linux-x64.AppImage
```

**Linux (.deb)**
```bash
sudo dpkg -i NiceSSH-v*-linux-x64.deb
```

## First-use walkthrough

1. **Open NiceSSH**. The app starts on the **Projects** view (empty).
2. Go to **Identities** → click **+ New Identity**.
   - Label: `Work`
   - User Name: `Your Name`
   - User Email: `work@company.com`
   - Key Path: `~/.ssh/id_work_ed25519` (default)
   - Match Path: `~/work` (so any repo under `~/work/` auto-uses this identity)
   - Host Alias / Git Host: leave as `github.com`
3. Click **Generate Key** on the new identity card → choose ED25519 → optionally set a passphrase.
   - The public key is copied to your clipboard.
4. Add the public key to your GitHub account: <https://github.com/settings/keys>
5. Go to **Projects** → click **+ Add Project** → select a git repo under `~/work/` → pick the `Work` identity → submit.
6. The project's `.git/config` is now set to use `id_work_ed25519`, and `~/.gitconfig` has a new `includeIf` block.
7. Click **Test SSH** to verify GitHub recognizes you as the work account.
8. **Push** to GitHub — it's that account, not your personal one.

Add a second identity (Personal) for `~/personal/`, and you're done.

## How it works under the hood

### Directory-based auto-matching (`includeIf`)

When you create an identity with a `Match Path` (e.g. `~/work`), NiceSSH appends this to your `~/.gitconfig`:

```ini
[includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work
```

And creates `~/.gitconfig-work` with:

```ini
[user]
    name = Your Name
    email = work@company.com
[core]
    sshCommand = ssh -i ~/.ssh/id_work_ed25519 -o IdentitiesOnly=yes
```

Git reads this automatically — no App process needed, no daemon, no extra hops.

### Per-project override

If a project is outside your `Match Path` (e.g. `~/random/`), you can still bind it to a specific identity from the **Projects** view. This writes directly to that project's `.git/config` and overrides any directory-level `includeIf`.

### SSH config preservation

NiceSSH only edits blocks it owns, marked with `# nicessh-managed`. Your hand-written blocks (`Host db-prod`, `Host *`, etc.) are preserved byte-for-byte. A new block is added, never replaces your file.

### Modification history + rollback

Every write to `~/.ssh/config`, `~/.gitconfig`, `~/.gitconfig-<label>`, and `~/.nicessh/config.json` is snapshotted to `~/.nicessh/history/` (last 50 entries kept). Open the **History** view, pick any entry, click **Revert** — your current state is auto-saved first, then restored.

## Development

NiceSSH is a [Tauri 2.x](https://tauri.app/) app: Rust backend + React 18 + TypeScript + Vite frontend.

### Prerequisites

- **Node.js 20+** and **pnpm 9+**
- **Rust 1.78+** (`rustup install stable`)
- **Tauri 2 prerequisites** for your OS — see <https://tauri.app/start/prerequisites/>

### Local setup

```bash
git clone https://github.com/e9ab98/NiceSSH.git
cd NiceSSH
pnpm install
cargo install tauri-cli --version "^2.0" --locked
cargo tauri dev
```

The first `cargo build` pulls ~1000 crates and takes 5–10 minutes. Subsequent builds are fast.

### Run tests

```bash
cd src-tauri
cargo test                          # Rust unit tests
cargo clippy --all-targets -- -D warnings   # Lints (must pass)
```

### Build a release binary for your platform

```bash
cargo tauri build
# → produces src-tauri/target/release/bundle/{dmg,msi,appimage,deb}/NiceSSH*
```

### Project structure

```
.
├── package.json              # Frontend deps
├── vite.config.ts            # Vite bundler config
├── tailwind.config.ts        # Tailwind with data-theme="dark" selector
├── index.html                # Anti-flash inline script for theme
├── src/                      # React frontend
│   ├── main.tsx
│   ├── App.tsx               # Sidebar + 6 routes
│   ├── components/
│   ├── views/                # Projects / Identities / SSH Keys / Config / History / Settings
│   ├── store/                # Zustand
│   ├── ipc/                  # Typed Tauri command wrappers
│   ├── i18n/                 # English / 简体中文 translation files
│   └── hooks/
└── src-tauri/                # Rust backend
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    └── src/
        ├── main.rs
        ├── lib.rs            # Tauri builder + IPC command registry
        ├── paths.rs          # ~/ expansion, file path resolution
        ├── fs_safety.rs      # atomic_write (tmp + rename + chmod)
        ├── history.rs        # 50-snapshot index + rollback
        ├── config_store.rs   # ~/.nicessh/config.json
        ├── ssh_config.rs     # parse + serialize (preserves user blocks)
        ├── git_config.rs     # includeIf appender + per-identity subfiles
        ├── ssh_keys.rs       # list + delete
        ├── runner.rs         # exec with 30s timeout
        └── commands/         # 23 #[tauri::command] handlers
```

### Adding translations

Strings live in `src/i18n/locales/{en,zh-CN}.json`. To add a new key, edit both files. The `useTranslation()` hook in components reads the active locale (selected in Settings → Language). New locales can be added by dropping a new JSON file and registering it in `src/i18n/index.ts`.

## Security & data handling

- **No network calls** other than what you explicitly trigger (e.g. the SSH test).
- **All filesystem writes** go through `atomic_write` (tmp file + rename), so a crash mid-write cannot leave a half-written config.
- **Passphrases** are never written to disk. They're held in the React state of the unlock dialog and passed to `ssh-add` once, then discarded.
- **SSH private keys** are only ever *read* by the app (to compute fingerprints), never copied or transmitted.
- The `~/.nicessh/` directory and its `history/` subdirectory contain only metadata + before/after diffs of public config files. Private key material is never included in any snapshot.

## Updates

NiceSSH checks for new releases once per app start (24h cache). When a
new version is available, a non-blocking toast appears with "Update
now" / "Later" / "×" buttons. Click **Update now** to download in the
background; once finished, the app asks you to close and reopen it to
apply the new version.

You can also check on demand: **Settings → Updates → Check now**. The
tab shows your current version, the latest available version, and a
toggle to opt out of update notifications.

### Caveats (pre-1.0.0)

- **macOS**: Auto-updated builds still trigger Gatekeeper on first
  open. Right-click → Open → "Open" to allow, same as a manual
  download. Notarization is v1.0.0 work.
- **Windows**: Auto-updated builds still trigger SmartScreen on first
  open. "More info" → "Run anyway" to allow, same as a manual
  download. EV code signing is v1.0.0 work.
- **Linux `.deb` / `.rpm`**: Updating requires `sudo` (the OS package
  manager is invoked under the hood). The `.AppImage` build is the
  recommended channel for auto-update on Linux — it updates in place
  without root.
- **Manual restart**: After a successful download, NiceSSH asks you to
  close and reopen the app to apply the update. (A one-click
  in-process relaunch via `@tauri-apps/plugin-process` is planned for
  a follow-up release; the manual close-and-reopen flow is identical
  from the user's perspective.)

## Limitations (MVP)

- **No in-app terminal.** NiceSSH does not replace your shell. It complements it: edit identities / projects in the GUI, then use `git` from the command line as usual.
- **No commit / push / pull UI.** The app shows the last 10 commits read-only, so you can verify "yes, this is the right identity before I push". The actual push still happens in your terminal.
- **No code signing / notarization at v0.1.0 / v0.3.0.** macOS Gatekeeper and Windows SmartScreen will show warnings. See [Release notes](CHANGELOG.md) for v1.0.0 plans.

## License

[MIT](LICENSE) — see the LICENSE file for full text.
