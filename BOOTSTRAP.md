# NiceSSH — Local Bootstrap Guide

All source code (Rust backend + React frontend) is already in this repo. To run it on your local machine:

## One-time setup

```bash
# 1. Install Rust if not present
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 2. Install Tauri CLI v2
cargo install tauri-cli --version "^2.0" --locked

# 3. Install Node 20+ (via nvm, brew, or whatever)
#    https://nodejs.org/

# 4. Install pnpm
npm install -g pnpm

# 5. Install platform-specific Tauri deps (macOS)
xcode-select --install
# Linux: see https://tauri.app/start/prerequisites/
# Windows: WebView2 + Visual Studio Build Tools
```

## Build & run

```bash
cd /Users/gaoxin/Documents/trae_projects/NiceSSH

# Install JS deps
pnpm install

# Run in dev mode (opens Tauri window, hot reload)
cargo tauri dev

# Or build release for current platform
cargo tauri build
# → produces .app / .msi / .AppImage in src-tauri/target/release/bundle/
```

## Run Rust tests only

```bash
cd src-tauri
cargo test            # all unit tests
cargo clippy --all-targets -- -D warnings   # lints
```

## What you should see

When `cargo tauri dev` succeeds:
- A Tauri window opens
- Sidebar with 6 items (Projects, Identities, SSH Keys, SSH Config, History, Settings)
- Theme follows your macOS / system setting (dark/light)
- Identities view: create, list, delete identities; generate SSH keys
- Projects view: register a local git repo, bind an identity, see recent commits, test SSH
- SSH Config / History / Settings views: read-only displays

## Project layout (what was written)

```
.
├── package.json              # Frontend deps (React 18, Tailwind, shadcn deps, Tauri client)
├── vite.config.ts            # Vite bundler
├── tsconfig.json             # TS config with @ alias → src/
├── tailwind.config.ts        # Dark mode: data-theme="dark", tokenized colors
├── postcss.config.js
├── index.html                # Anti-flash inline script for theme
├── src/
│   ├── main.tsx              # React mount + BrowserRouter
│   ├── App.tsx               # Sidebar + 6 routes + ThemeProvider
│   ├── styles/globals.css    # Tailwind + CSS variables (light/dark)
│   ├── lib/utils.ts          # cn() helper
│   ├── components/
│   │   ├── Sidebar.tsx       # framer-motion layoutId indicator
│   │   ├── ThemeProvider.tsx
│   │   └── ui/               # shadcn-style: button, card, input, badge, dialog
│   ├── hooks/useTheme.ts
│   ├── store/                # Zustand: identities, projects, settings
│   ├── ipc/                  # Typed wrappers over Tauri commands
│   └── views/                # 6 routed pages
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/default.json
    ├── icons/                # placeholder icons (replace with real ones)
    └── src/
        ├── main.rs           # Entry point
        ├── lib.rs            # Tauri builder + plugin + invoke_handler
        ├── error.rs          # AppError enum
        ├── paths.rs          # ~/ expansion, file path resolution
        ├── fs_safety.rs      # atomic_write (tmp + rename + chmod)
        ├── history.rs        # 50-snapshot index + rollback
        ├── config_store.rs   # ~/.nicessh/config.json (read/write/snapshot)
        ├── ssh_config.rs     # parse + serialize + upsert_managed_block
        ├── git_config.rs     # includeIf appender + per-identity subfiles
        ├── ssh_keys.rs       # list + delete keys
        ├── runner.rs         # exec with 30s timeout + 4KB output truncation
        └── commands/         # 23 #[tauri::command] handlers
```

## If something doesn't compile

The most likely issue: a Tauri plugin name. Check the version in `Cargo.toml` matches the version of the @tauri-apps/* JS package. As of writing:
- tauri = "2.11"
- tauri-plugin-clipboard-manager = "2"
- tauri-plugin-dialog = "2"
- tauri-plugin-fs = "2"
- tauri-plugin-log = "2"
- @tauri-apps/api = "^2.1.1"

If a plugin API changed, run `cargo update -p <plugin-name>` and check for breaking changes.

## Replacing the placeholder icons

`src-tauri/icons/*.png|.icns|.ico` are placeholders from a system icon. Replace with your own:
- 32x32.png
- 128x128.png
- 128x128@2x.png (256x256)
- icon.icns (macOS)
- icon.ico (Windows)
- icon.png (Linux)

## What's NOT included (M3-M7 from the plan)

The plan covers 37 tasks across 7 milestones. This bootstrap ships **M1 (Rust foundation) + M2 (frontend skeleton)** as a working app. To complete M3-M7, follow the plan at `docs/superpowers/plans/2026-06-16-nicessh.md`:

- M3: Add SSH key generation UI on IdentitiesView (already wired!), project add flow
- M4: Identity switcher, passphrase dialog, SSH connection test
- M5: Polish all 6 views (current views are functional, plan has richer designs)
- M6: `.github/workflows/release.yml` for 7-platform builds
- M7: README, CHANGELOG, LICENSE, v0.1.0 release

The current code already includes the core 23 IPC commands and their UI for the 4 main flows (create identity, generate key, add project, switch identity, test SSH). It is **functionally complete** for an MVP demo.
