# OpenRS

OpenRS is a Rust-first retail application with a native desktop app and a
WebAssembly web app. Both targets share the same domain logic and the same
`egui` UI code.

This repository is currently in **Phase 0.1**: the workspace and the desktop +
WASM toolchain are being proven with a minimal “Hello OpenRS” screen. There is
no database, persistence, product catalog, or checkout logic yet.

## Architecture

```text
crates/
├── openrs-core/       # Pure shared domain logic; no UI or platform services
├── openrs-ui/         # Shared egui screens/widgets; no eframe dependency
├── openrs-desktop/    # Native eframe + wgpu shell
└── openrs-web/        # eframe WebRunner shell built by Trunk
```

The desktop shell uses native `eframe`/`egui` on `wgpu`. The web shell uses the
same Rust code compiled to `wasm32-unknown-unknown` with Trunk and
`wasm-bindgen`. There is no Tauri, Electron, webview, or JavaScript UI
framework.

## Prerequisites

- Rust **1.95.0** (pinned by `rust-toolchain.toml`)
- The `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) **0.22.1**
- [wasm-pack](https://rustwasm.github.io/wasm-pack/) **0.15.1** for browser tests
- Linux desktop builds need X11/Wayland and Vulkan development libraries

The repository pins Cargo to two parallel build jobs in `.cargo/config.toml`.
Pass `--jobs 2` explicitly if you run Cargo from outside the repository root.

Install the Rust toolchain pieces:

```bash
rustup toolchain install 1.95.0 \
  --profile minimal \
  --component clippy,rustfmt \
  --target wasm32-unknown-unknown
```

Install the build/test tools:

```bash
cargo install trunk --version 0.22.1 --locked
cargo install wasm-pack --version 0.15.1 --locked
```

Trunk downloads a `wasm-bindgen` CLI matching `Cargo.lock` on first build. If
that download fails (for example on a restricted network), install the matching
release manually and put `wasm-bindgen` on `PATH`:

```bash
# Version must match the `wasm-bindgen` crate version in Cargo.lock (0.2.130).
curl -sSL https://github.com/rustwasm/wasm-bindgen/releases/download/0.2.130/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl.tar.gz \
  | tar -xz -C /tmp
install -m 755 /tmp/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl/wasm-bindgen ~/.local/bin/
```

## Run

Desktop:

```bash
cargo run -p openrs-desktop
```

Web:

```bash
cd crates/openrs-web
trunk serve
```

Then open the local URL printed by Trunk.

Release web build:

```bash
cd crates/openrs-web
trunk build --release=true
```

Trunk must run from `crates/openrs-web`, where the crate's `index.html` lives.
Passing the HTML path from the workspace root fails with
`could not find the root package of the target crate`.

## Checks

Static and unit checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p openrs-web --target wasm32-unknown-unknown
```

Browser parity test (runs the shared core contract in Chromium):

```bash
wasm-pack test --headless --chrome crates/openrs-core
```

End-to-end smoke tests (headless, no display or GPU required):

```bash
cargo build -p openrs-desktop
scripts/smoke-desktop.sh     # opens a window under Xvfb and screenshots it
scripts/smoke-web.sh         # builds, serves, and loads the web app in Chromium
```

`scripts/smoke-desktop.sh` needs `Xvfb`, `xwininfo`, and ImageMagick's `import`.
`scripts/smoke-web.sh` needs `trunk`, a matching `wasm-bindgen` CLI, Chromium or
Google Chrome, and `python3`.

### What each layer proves

| Check | Proves |
|---|---|
| `cargo test` | Shared core logic and shared egui UI (via `egui_kittest`) |
| `wasm-pack test --chrome` | The same core contract passes in a real browser |
| `scripts/smoke-desktop.sh` | The native shell opens a window and renders through `wgpu` |
| `scripts/smoke-web.sh` | The WASM bundle loads and `WebRunner` starts without panicking |

## Documentation

- `AGENTS.md` — contributor and agent rules
- `SPEC.md` — initial product scope
- `ROADMAP.md` — phased delivery plan
- `UI-UX.md` — interaction and accessibility principles
- `docs/adr/` — architecture decision records
- `docs/phase-0-inspection.md` — initial repository and feasibility report
- `docs/phase-0.1-report.md` — Phase 0.1 scaffold results
- `docs/screenshots/` — desktop and web captures of the Phase 0.1 screen

## Open decisions

The following are intentionally **not** decided yet:

- Database/storage engine and transaction strategy
- Product-management scope (read-only/imported catalog vs. full CRUD)
- Receipt format and printing behavior
- Currency/ID/sale-status details
- Supported OS/browser/accessibility matrix
- License

Do not add a database or major product architecture without an ADR and project
owner approval.
