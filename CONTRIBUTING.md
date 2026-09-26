# Contributing to clAIre

Thanks for helping. clAIre is a small Tauri desktop app: a React frontend in `src/` and a Rust backend in `src-tauri/`. Keep changes focused, and match the code around them.

By participating, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Set up the build

You need:

- **Node.js** 18 or newer
- **Rust** stable via [rustup](https://sh.rustup.rs/) (`rust-toolchain.toml` pins the `stable` channel; 1.77.2 or newer)
- Platform packages for Tauri 2 (WebKitGTK on Linux, Xcode Command Line Tools on macOS, WebView2 and the MSVC build tools on Windows)

Linux, macOS, and Windows package lists are in the [README](README.md#requirements). After those are installed:

```bash
# Rust toolchain (once per machine)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Frontend dependencies
npm install

# App with hot reload (first Rust compile can take several minutes)
npm run tauri dev
```

Other useful commands:

| Command | Purpose |
| --- | --- |
| `npm run tauri dev` | Overlay (including the settings view) and Rust backend |
| `npm run tauri build` | Production installer under `src-tauri/target/release/bundle/` |
| `npm test` | Frontend unit tests (Vitest) |
| `npm run lint` | ESLint |
| `npm run build` | Typecheck and build the frontend only |
| `npm run icons` | Regenerate tray and app icons from `scripts/gen_icons.py` |

On macOS, grant Screen Recording and Accessibility to the dev binary (or to Terminal) or hotkeys and screenshots will not work.

## Checks before you open a pull request

GitHub Actions runs these checks on every pull request and on every push to `main`. Run the same commands locally, and say in the pull request how you exercised the change.

```bash
npm test
npm run lint
npm run build
cargo fmt --all -- --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

`npm run build` runs `tsc` with the strict settings in `tsconfig.json` (`strict`, `noUnusedLocals`, `noUnusedParameters`). CI runs Clippy as `cargo clippy --all-targets` (warnings stay warnings). Rust tests live in `#[cfg(test)]` modules next to the code.

For anything a user can see or trigger, also run `npm run tauri dev` and try the path you changed: hotkey, overlay, settings, capture, or a provider call. Do not paste API keys, `.env` contents, or screenshots of private screens into the pull request or an issue.

## Pull requests

1. Fork the repository and branch from `main`.
2. Keep the pull request to one change. A drive-by refactor belongs in its own pull request.
3. Open the pull request against `main`. Fill in the template: what changed, why, and how you checked it.
4. A maintainer will review. Small, clear diffs get reviewed faster.

Do not commit secrets. API keys belong in the system credential store (through Settings) or a local `.env` that is not part of the diff.

## Code style

Follow the file you are editing. Do not reformat unrelated code.

**TypeScript / React** (`src/`)

- Function components and plain modules, as in `src/overlay/`, `src/settings/`, and `src/shared/`.
- Shared IPC wrappers live in `src/shared/api.ts`. Shared types live in `src/shared/types.ts`.
- Let `tsc` catch unused locals and parameters. ESLint covers `src/` via `npm run lint`. There is no Prettier config.

**Rust** (`src-tauri/`)

- Edition 2021. Format with `cargo fmt` (default rustfmt).
- Chat providers are `Provider` in `src-tauri/src/settings.rs`, with request code in `src-tauri/src/llm.rs`. Web search providers are `SearchProvider` in `settings.rs`, with request code in `src-tauri/src/search.rs`.
- Prefer a small change in the existing module over a new abstraction.

## Reporting bugs and ideas

Use the GitHub issue forms:

- **Bug report** for a broken hotkey, capture, window, or provider call
- **Feature request** for a product change
- **Provider integration** for a new chat or search provider

Questions about local data and screenshots belong in [PRIVACY.md](PRIVACY.md), or by email to voltrexium@gmail.com.
