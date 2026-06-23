# Contributing to NiceSSH

Thanks for your interest in contributing! NiceSSH is a small project and contributions of all sizes are welcome.

## Development setup

### Prerequisites
- **Node.js 20+** and **pnpm 9+**
- **Rust 1.78+** (`rustup install stable`)
- **Tauri 2.x CLI**: `cargo install tauri-cli --version "^2.0" --locked`
- **Tauri 2 OS prerequisites** — see <https://tauri.app/start/prerequisites/>

### First-time setup
```bash
git clone https://github.com/e9ab98/NiceSSH.git
cd NiceSSH
pnpm install
cargo tauri dev    # launches the app in dev mode with hot reload
```

The first `cargo build` pulls ~1000 crates and takes 5-10 minutes. Subsequent builds are incremental and fast.

## Running tests

```bash
cd src-tauri
cargo test                          # Rust unit tests (~30 tests)
cargo clippy --all-targets -- -D warnings    # Lints (must be clean)
```

Frontend type-check:
```bash
pnpm tsc --noEmit
```

## Conventions

### Commits
[Conventional Commits](https://www.conventionalcommits.org/):
- `feat: <description>` — new feature
- `fix: <description>` — bug fix
- `test: <description>` — test-only change
- `docs: <description>` — documentation only
- `chore: <description>` — tooling, deps, etc.
- `refactor: <description>` — internal change with no behavior change

### Rust style
- `cargo clippy --all-targets -- -D warnings` must pass
- Module-per-responsibility: prefer small focused files
- Public APIs documented with `///` rustdoc
- Tests live in the same file as the code they test, under `#[cfg(test)] mod tests`

### Frontend style
- TypeScript strict mode (enforced by `tsconfig.json`)
- Components are functional; hooks for stateful logic
- Avoid `any`; type everything
- Tailwind utility classes for layout; CSS variables for colors
- shadcn/ui pattern: copy components into `src/components/ui/`, customize locally

### Pull requests
1. Branch from `main`: `git checkout -b feat/your-feature`
2. One logical change per PR
3. Reference any related issues (`Fixes #123`)
4. Include before/after screenshots for any UI change
5. CI must pass (rust test + clippy + tsc)
6. Squash-merge with a Conventional Commit message

## Release process

1. Update version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
2. Move "Unreleased" items in `CHANGELOG.md` to a dated `## [X.Y.Z] - YYYY-MM-DD` section
3. Commit: `chore(release): bump version to X.Y.Z`
4. Tag: `git tag vX.Y.Z && git push --tags`
5. GitHub Actions `.github/workflows/release.yml` builds 7+ artifacts and publishes a GitHub Release automatically

## Code of conduct

Be kind, be constructive, assume good faith. This is a small tool for a specific pain point — let's keep it that way.

## Questions?

Open a GitHub issue. We check them.
