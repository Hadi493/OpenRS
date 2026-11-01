# Phase 0.1 — Workspace and toolchain proof

**Date:** 2026-11-02
**Status:** Complete — all acceptance criteria verified locally
**Scope:** Workspace scaffold, shared “Hello OpenRS” screen, native and web
shells, checks, CI, and documentation. **No product features, no database, no
persistence.**

---

## 1. What was built

```text
OpenRetail/
├── Cargo.toml                     # workspace, resolver 3, pinned deps, lints
├── Cargo.lock                     # committed
├── rust-toolchain.toml            # Rust 1.95.0 + wasm32-unknown-unknown
├── rustfmt.toml
├── .cargo/config.toml             # jobs = 2 (project rule)
├── .gitignore                     # .LOCAL/, target/, dist/, screenshot diffs
├── README.md
├── AGENTS.md  SPEC.md  ROADMAP.md  UI-UX.md
├── crates/
│   ├── openrs-core/               # pure shared domain logic
│   │   ├── src/lib.rs             # PRODUCT_NAME, foundation_message()
│   │   └── tests/
│   │       ├── foundation_contract.rs   # native contract test
│   │       └── wasm_parity.rs           # same contract in a browser
│   ├── openrs-ui/                 # shared egui UI (egui only, no eframe)
│   │   └── src/lib.rs             # OpenRsApp::ui + egui_kittest test
│   ├── openrs-desktop/            # native eframe + wgpu shell
│   └── openrs-web/                # eframe WebRunner shell + index.html
├── scripts/
│   ├── smoke-desktop.sh           # Xvfb window + screenshot check
│   └── smoke-web.sh               # build + serve + headless browser check
├── docs/
│   ├── adr/0001-workspace-and-targets.md
│   ├── phase-0-inspection.md
│   ├── phase-0.1-report.md        # this file
│   └── screenshots/
│       ├── phase-0.1-desktop.png
│       └── phase-0.1-web.png
└── .github/workflows/ci.yml
```

### Dependency boundaries

| Crate | Depends on | Must never depend on |
|---|---|---|
| `openrs-core` | nothing (dev: `wasm-bindgen-test`) | `egui`, `eframe`, filesystem, network |
| `openrs-ui` | `egui`, `openrs-core` | `eframe`, platform services |
| `openrs-desktop` | `eframe` (wgpu), `env_logger`, `openrs-ui` | business rules |
| `openrs-web` | `eframe` (wgpu), `log`, `web-sys`, `openrs-ui` | business rules |

The shells contain only window/canvas startup and a two-line delegation to
`openrs_ui::OpenRsApp::ui`. All user-visible text comes from `openrs-core`.

### Pinned versions

| Component | Version |
|---|---|
| Rust / Cargo | 1.95.0 |
| `eframe` / `egui` / `egui_kittest` | 0.37.0 |
| `wgpu` (transitive) | 30.0.1 |
| `wasm-bindgen` (crate + CLI) | 0.2.130 |
| Trunk | 0.22.1 |
| wasm-pack | 0.15.1 |
| Chromium / ChromeDriver | 156.0.8078.73 |

`eframe` is declared with `default-features = false` and features
`accesskit`, `default_fonts`, `wayland`, `web_screen_reader`, `wgpu`, `x11`.
`glow` is deliberately not enabled; the renderer is set to
`eframe::Renderer::Wgpu` explicitly in both shells.

---

## 2. Verification results

Every command below was run in this repository on 2026-11-02 and passed.

| # | Command | Result |
|---|---|---|
| 1 | `cargo fmt --all --check` | exit 0 |
| 2 | `cargo clippy --workspace --all-targets --jobs 2 -- -D warnings` | exit 0, no warnings |
| 3 | `cargo test --workspace --jobs 2` | exit 0 — 2 core unit tests, 1 native contract test, 1 `egui_kittest` UI test |
| 4 | `cargo check -p openrs-web --target wasm32-unknown-unknown --jobs 2` | exit 0 |
| 5 | `cargo check -p openrs-desktop --target wasm32-unknown-unknown --jobs 2` | exit 0 |
| 6 | `cd crates/openrs-web && trunk build --release=true` | exit 0 — `dist/` contains `index.html`, `*.js`, `*.wasm` (6.2 MB) |
| 7 | `wasm-pack test --headless --chrome crates/openrs-core` | exit 0 — `shared_foundation_contract_runs_in_wasm ... ok` |
| 8 | `cargo build -p openrs-desktop --jobs 2` | exit 0 |
| 9 | `scripts/smoke-desktop.sh` | `PASS: 'OpenRS' window rendered` |
| 10 | `scripts/smoke-web.sh` | `PASS: web app started in chromium` |

### What the two smoke tests prove

The unit and `egui_kittest` tests never touch a real window or a real browser, so
two additional checks close that gap:

- **`scripts/smoke-desktop.sh`** starts `Xvfb`, launches the binary with
  `WGPU_BACKEND=gl`, asserts that a window titled `OpenRS` is mapped, and
  captures a screenshot. In this environment `wgpu` selected Mesa `llvmpipe`
  (`device_type: Cpu`), so the test works without a GPU.
- **`scripts/smoke-web.sh`** builds with Trunk, serves `dist/` over loopback,
  loads the page in headless Chromium with SwiftShader, and asserts that
  `#loading_text` is gone. That element is removed only after
  `WebRunner::start` resolves successfully, so its absence is a real
  startup-success signal rather than a mere page load.

Both screenshots are committed under `docs/screenshots/`. Desktop and web show
the same three lines (`OpenRS`, `OpenRS foundation is running`,
`Phase 0.1 foundation smoke test`), which is the first visible parity evidence.
The only visible difference is the theme (desktop follows the OS dark
preference, headless Chromium reports light); that is an egui theme choice, not
a business-behavior difference.

### Problems found and fixed during verification

| Problem | Cause | Fix |
|---|---|---|
| `trunk build --release crates/openrs-web/index.html` failed with `could not find the root package of the target crate` | Trunk resolves the crate from the working directory, and `--release` takes an optional boolean value | Run Trunk from `crates/openrs-web` and use `--release=true` |
| Trunk build failed downloading the `wasm-bindgen` CLI | Network timeout fetching the release | Installed `wasm-bindgen` 0.2.130 manually; documented the workaround in README |
| `cannot find macro 'wasm_bindgen_test_configure'` | The macro is a separate import | `use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};` |
| `error` type in `panic!("{error}")` did not implement `Display` on wasm | `eframe::web::WebRunner` error type | Formatted with `{error:?}` |
| Desktop smoke test captured a black screen and the virtual display had no windows | `winit` preferred the host `WAYLAND_DISPLAY` over `Xvfb` | Clear `WAYLAND_DISPLAY`/`WAYLAND_SOCKET` and set `WINIT_UNIX_BACKEND=x11` in the script |

---

## 3. Deviations from the original proposal

- **Smoke tests were added.** The proposal listed only `cargo test`,
  `wasm-pack test`, and a manual desktop launch. Manual verification is not
  reproducible in CI, so both checks are now scripts wired into CI.
- **`CHANGELOGS.md` was left untouched.** It is an empty, user-created file with
  an inconsistent name. Renaming it to `CHANGELOG.md` is a release-time task and
  was not done without approval.
- **Root `AGENT.md` was removed** after copying its content to `AGENTS.md`, since
  it was empty and would shadow the real guide.

---

## 4. Known limitations

1. **Web bundle size.** The release wasm is 6.2 MB before any application code.
   `wgpu` dominates. Compression and load-time measurement are required before
   shipping; this is tracked as a Phase 6 concern.
2. **WebGPU is not universal.** The web shell requests `wgpu`, which uses WebGPU
   when available and falls back to WebGL. Both paths must be tested before
   release; only the WebGL path was exercised here (SwiftShader).
3. **Accessibility on web.** `web_screen_reader` is enabled, but egui renders to
   a canvas. Real screen-reader behavior has not been verified against
   `UI-UX.md` requirements.
4. **CI is unverified in GitHub Actions.** The workflow parses and mirrors the
   commands proven locally, but it has never run on a GitHub runner. The desktop
   job depends on Mesa `llvmpipe` and the web job on the runner's preinstalled
   Chrome.
5. **No packaging.** There is no desktop installer, no PWA/offline web support,
   and no `wasm-opt` size tuning beyond Trunk's default.
6. **License is still undecided.** No `LICENSE` file exists.

---

## 5. Constraints confirmed

- No Tauri, Electron, webview shell, or JS/TS UI framework in the dependency
  graph.
- No handwritten JavaScript. `crates/openrs-web/index.html` is static HTML/CSS;
  the only script in `dist/` is wasm-bindgen/Trunk-generated glue.
- `cargo check -p openrs-desktop --target wasm32-unknown-unknown` passes, so the
  shared crates stay target-neutral.
- All business-visible strings originate in `openrs-core` and are asserted by
  tests on both targets.

---

## 6. Recommended next step

**Phase 0.2 — storage spike (timeboxed), not a feature.**

`SPEC.md` requires atomic checkout, idempotent retry, and historical price
snapshots. Those are storage properties, so the engine decision blocks Phase 1.
The inspection report recommends spiking **SQLite everywhere** (`rusqlite`
natively, `sqlite-wasm-rs` + OPFS on web) before committing, and comparing it
with **SQLite native + IndexedDB web** behind one repository trait.

Definition of done for the spike:

- One `Repository` trait with an in-memory implementation used by tests.
- A native SQLite implementation with a real transaction and a unique
  idempotency key.
- Evidence for the web side: OPFS persistence, browser support, header
  requirements (COOP/COEP), and bundle-size impact.
- A short ADR choosing the engine, with the rejected option documented.

Still open and requiring project-owner decisions before Phase 1: license,
product-management scope (read-only/imported catalog vs. CRUD), money/ID/sale
status details, receipt format, and the support matrix.
