# OpenRS

OpenRS is a Rust-first retail application with a native desktop app and a
WebAssembly web app. Both targets share the same domain logic and the same
`egui` UI code.

This repository is currently at **Phase 2: bill creation and calculation**, with
the Phase 1 catalog complete. The workspace, the desktop + WASM toolchain, and
the storage layer are implemented and tested, and both shells render the shared
catalog screen: products, variants, prices, and stock availability, with text
search and category filtering.

Both shells load that catalog from SQLite at startup (ADR 0003). The first run
seeds the development sample into an empty database and every later run reads
what is stored, so a change made by one session is what the next session sees.
A failed startup shows the reason on screen instead of an empty catalog.

An available variant can be added to the current bill. The bill panel shows each
line's quantity, unit price, and line total, lets the user change quantities or
remove lines, and shows the exact subtotal required by `BILL-001`. Stock and
currency rules are enforced in shared Rust before the bill changes, and a
rejected change is explained in the panel. The bill is in-memory only: completing
and recording the sale is Phase 3 and is not wired up yet.

Storage is SQLite on both targets (ADR 0002): `rusqlite` natively,
`sqlite-wasm-rs` + OPFS in the browser. `crates/openrs-storage` owns the schema,
the repository contract, and the sale guarantees; neither shell nor `openrs-ui`
contains SQL.

## Architecture

```text
crates/
├── openrs-core/       # Shared domain logic: catalog types, listing rules, bill rules, sample data
├── openrs-storage/    # Repository contract, SQLite schema, sale guarantees
├── openrs-ui/         # Shared egui screens/widgets; no eframe dependency
├── openrs-desktop/    # Native eframe + wgpu shell
├── openrs-web/        # eframe WebRunner shell built by Trunk
└── openrs-web-worker/ # Dedicated worker that owns the browser SQLite database
```

The browser database lives in `openrs-web-worker`, because OPFS file handles
only exist inside a dedicated worker. The web shell starts it by URL, waits for
its single startup message, and hands the decoded catalog to the shared UI.

Catalog value types (`Product`, `Variant`, `MinorUnits`) are defined in
`openrs-core` and re-exported by `openrs-storage::model`, so the UI and storage
cannot drift apart. Sale value types still live in `openrs-storage`.

`openrs-storage` is the only crate allowed to use `unsafe`, and only in
`src/sqlite/wasm.rs`, because the browser driver calls the SQLite C API. Every
other crate keeps the workspace `unsafe_code = "forbid"`.

The desktop shell uses native `eframe`/`egui` on `wgpu`. The web shell uses the
same Rust code compiled to `wasm32-unknown-unknown` with Trunk and
`wasm-bindgen`. There is no Tauri, Electron, webview, or JavaScript UI
framework.

## Prerequisites

- Rust **1.95.0** (pinned by `rust-toolchain.toml`)
- The `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) **0.22.1**
- [wasm-pack](https://rustwasm.github.io/wasm-pack/) **0.15.1** for browser tests
- Node.js **18+** for `scripts/smoke-web.sh`, which drives Chromium over the
  DevTools protocol
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
cargo check -p openrs-web-worker --target wasm32-unknown-unknown
```

Browser parity tests (the same contracts run in Chromium):

```bash
wasm-pack test --headless --chrome crates/openrs-core
wasm-pack test --headless --chrome crates/openrs-storage
```

The core run includes the catalog listing contract in a browser. The storage run
includes the guarantee suite in a browser and the OPFS persistence tests in a
dedicated worker.

### Storage deployment notes

- Native uses WAL with `synchronous = FULL`. OPFS does **not** support WAL, so
the browser uses the rollback journal; the SQL and transaction model are
identical.
- OPFS sync access handles only exist in a dedicated worker, so the browser
database lives in `openrs-web-worker` and the UI reaches it through a single
`postMessage` (ADR 0003).
- The web deployment must send `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` because the OPFS VFS uses
`SharedArrayBuffer`. `Trunk.toml` sets them for `trunk serve`, the `wasm-pack`
test server and `scripts/smoke-web.sh` send them; a plain static host needs
configuring.
- `Trunk.toml` sets `filehash = false` so the worker script keeps the stable
name `./openrs-web-worker_loader.js` that the app starts by URL.
- Storage adds roughly 0.5 MB gzipped to the WASM bundle (measured in
`docs/phase-0.2-storage-spike-report.md`).

End-to-end smoke tests (headless, no display or GPU required):

```bash
cargo build -p openrs-desktop
scripts/smoke-desktop.sh     # opens a window under Xvfb and screenshots it
scripts/smoke-web.sh         # builds, serves, and loads the web app in Chromium
```

`scripts/smoke-desktop.sh` needs `Xvfb`, `xwininfo`, and ImageMagick's `import`.
`scripts/smoke-web.sh` needs `trunk`, a matching `wasm-bindgen` CLI, Chromium or
Google Chrome, Node.js, and `python3`.

### What each layer proves

| Check | Proves |
|---|---|
| `cargo test` | Shared core logic, catalog listing and bill rules (`BILL-001`), the shared catalog + bill screen and its search/filter/add/quantity interactions (via `egui_kittest`), and the storage guarantees against both backends |
| `cargo test -p openrs-storage --test native_file` | Sales survive a file reopen and concurrent checkouts cannot duplicate or oversell |
| `wasm-pack test --chrome` | The same catalog, bill, and storage contracts pass in a real browser |
| `scripts/smoke-desktop.sh` | The native shell opens a window, seeds the database on first run, and renders through `wgpu` |
| `scripts/smoke-web.sh` | The WASM bundle loads, `WebRunner` starts, and the storage worker opens OPFS and reports a loaded catalog in a real browser |

## Documentation

- `AGENTS.md` — contributor and agent rules
- `SPEC.md` — initial product scope
- `ROADMAP.md` — phased delivery plan
- `UI-UX.md` — interaction and accessibility principles
- `docs/adr/` — architecture decision records
- `docs/phase-0-inspection.md` — initial repository and feasibility report
- `docs/phase-0.1-report.md` — Phase 0.1 scaffold results
- `docs/phase-0.2-storage-spike-report.md` — storage spike evidence
- `docs/phase-0.3-storage-layer-report.md` — implemented storage layer results
- `docs/phase-1-catalog-report.md` — Phase 1 product catalog results
- `docs/phase-1.1-catalog-persistence-report.md` — catalog startup on SQLite results
- `docs/phase-2-bill-report.md` — Phase 2 bill creation and calculation results
- `docs/adr/0002-storage-engine.md` — accepted storage engine decision
- `docs/adr/0003-catalog-startup-and-web-worker.md` — accepted catalog startup and worker decision
- `spikes/` — throwaway storage investigations; not product code
- `docs/screenshots/` — desktop and web captures of the Phase 0.1 screen

## Open decisions

The following are intentionally **not** decided yet:

- Database/storage engine and transaction strategy — **decided**: SQLite on
  both targets (ADR 0002), implemented in `crates/openrs-storage`
- Product-management scope (read-only/imported catalog vs. full CRUD)
- Whether `Product.active` and variant attributes (size, color) are stored as
  columns, and the schema migration that would need
- Receipt format and printing behavior
- Currency/ID/sale-status details
- Supported OS/browser/accessibility matrix
- License

Do not add a database or major product architecture without an ADR and project
owner approval.
