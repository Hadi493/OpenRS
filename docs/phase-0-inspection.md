# OpenRS Phase 0 Inspection Report

**Date:** 2026-11-02  
**Status:** Inspection only — no application code, dependencies, or architecture decisions were added.

I inspected the repository and ran read-only checks. No repository files were created or modified as part of the inspection itself. This report is the saved output of that inspection.

**Bottom line:** the project documents exist, but they live in an ignored `.LOCAL` folder, the root is not a Git repository, and there is no Rust code. Nothing currently builds or tests. The proposed `egui/eframe + wgpu + Trunk/wasm-bindgen` stack is technically plausible, but it is not yet proven in this repo. The largest risks are **storage/transactions**, **receipt printing/export**, and **undefined product scope** — not the UI stack itself.

---

## 1. What files and structure exist

### Root

```text
/home/hadi/Programming/projects/OpenRetail/
├── AGENT.md          # 0 bytes
├── CHANGELOGS.md     # 0 bytes
├── .gitignore        # contains only ".LOCAL/"
├── docs/             # empty
└── .LOCAL/
    ├── AGENT.md      # 66 lines, actual project guide
    ├── SPEC.md       # 130 lines
    ├── ROADMAP.md    # 94 lines
    └── UI-UX.md      # 135 lines
```

Important details:

- The root directory is **not** a Git repository.
- `.LOCAL/` is a separate Git repository with one commit (`4eb6f15 Initial commit`), clean working tree, and no remote.
- `.gitignore` contains only `.LOCAL/`, so if the root is initialized as a Git repo, **the actual project documents will be ignored**.
- `AGENT.md` at the root is empty. The real guide is `.LOCAL/AGENT.md`, whose first line is `# AGENTS.md: guide for AI agents and contributors`.
- There is no `Cargo.toml`, `src/`, `tests/`, `.github/`, `README`, `LICENSE`, `rust-toolchain.toml`, or build configuration.
- The product is called **OpenRS**, while the directory is `OpenRetail`. That may be intentional, but package/binary naming should be decided.

### Document summary

- **AGENTS.md**: Rust-only application logic; no Tauri, Electron, webviews, or JS/TS UI frameworks; desktop uses egui/eframe on wgpu; web uses Rust-to-WASM via Trunk + wasm-bindgen; TDD, small steps, parity, ask before major decisions.
- **SPEC.md**: product/variant/sale data; exact money representation; product listing, bill creation/calculation, atomic sale completion, inventory update, idempotent checkout, receipt, and history. `BILL-001` currently covers subtotal only.
- **ROADMAP.md**: Phase 0 foundations, Phase 1 catalog, Phase 2 bill, Phase 3 sale completion, Phase 4 receipts/history, Phase 5 stock alerts, Phase 6 release readiness. Database choice is explicitly left open in Phase 0.
- **UI-UX.md**: common-workflow-first design, visible state, accessibility, keyboard support, parity, and several open design decisions.

---

## 2. Does the project build or test?

No. There is no Rust project yet, so the commands fail before compilation.

| Command | Result |
|---|---|
| `cargo metadata --no-deps` | `error: could not find Cargo.toml ...` — exit `101` |
| `cargo check` | same error — exit `101` |
| `cargo test` | same error — exit `101` |
| `cargo fmt --check` | cargo metadata error before formatting — exit `1` |
| `cargo clippy -- -D warnings` | same `Cargo.toml` error — exit `101` |

This is **not** a failing test suite; there is simply no crate or workspace to check.

### Toolchain and environment

| Item | Status |
|---|---|
| `rustc` / `cargo` | `1.95.0` |
| `rustfmt` | `1.9.0-stable` |
| `clippy` | `0.1.95` |
| Installed targets | `x86_64-unknown-linux-gnu`, `wasm32-unknown-unknown` |
| `trunk` | **not installed** |
| `wasm-bindgen` CLI | **not installed** |
| `wasm-pack` | **not installed** |
| Chromium / ChromeDriver | `156.0.8078.73` (matching versions) |
| Firefox | `159.0` |
| `xvfb-run` | available |
| `cargo-nextest` | `0.9.148` available |
| `cargo-deny` | `0.20.2` available |
| Linux UI libs | X11 `1.8.14`, xkbcommon `1.13.2`, Wayland client `1.26.0`, Vulkan `1.4.363`, GTK3 `3.24.52` |

Current relevant crate versions from crates.io as of today:

- `eframe` / `egui` / `egui_kittest`: **0.37.0**
- `wgpu`: **30.0.1**
- `wasm-bindgen`: **0.2.130**
- `trunk`: **0.22.1**
- `wasm-pack`: **0.15.1**

`eframe 0.37.0` declares `rust-version = "1.95"`, which matches the installed compiler exactly. That is convenient, but it should be pinned.

---

## 3. Conflicts, missing decisions, and feasibility risks

### 3.1 Documentation and repository governance

| Issue | Impact | Recommendation |
|---|---|---|
| Root is not a Git repo; `.LOCAL` is the only repo | Code and docs can diverge; a fresh clone may have no specs | Initialize Git at the root and track the project docs |
| `.gitignore` ignores `.LOCAL/` | The actual `SPEC.md`, `ROADMAP.md`, `UI-UX.md`, and guide are untracked | Move or copy the canonical docs out of `.LOCAL`; keep `.LOCAL` for private notes only |
| Root `AGENT.md` is empty; real guide is `.LOCAL/AGENT.md` | Agents may follow an empty file | Rename to `AGENTS.md` and place it at the root |
| `docs/` is empty | No ADRs, setup docs, or decision record location | Use `docs/adr/` for architecture decisions |
| `CHANGELOGS.md` is empty and plural | Low impact, but inconsistent | Rename to `CHANGELOG.md` and populate at release time |
| No `README`, `LICENSE`, or build instructions | Contributors cannot build or know licensing | Add both during Phase 0; license choice needs approval |
| AGENTS.md naming rule is incomplete | Line 3 ends with ``(`{Function}OpenRS` in PascalCase.`` | Clarify what identifiers should look like |

**Recommendation:** make the root the canonical Git repository, track `AGENTS.md`, `SPEC.md`, `ROADMAP.md`, `UI-UX.md`, and `docs/`, and leave `.LOCAL/` ignored for private working notes. This should be confirmed before any code is added.

### 3.2 Native desktop + WebAssembly feasibility

The stack is viable:

- `eframe 0.37.0` supports native and `wasm32-unknown-unknown`.
- Its `wgpu` feature enables `egui-wgpu`; web defaults to WebGPU with WebGL fallback.
- The `eframe_template` project demonstrates a Trunk setup where `index.html` is static HTML/CSS plus a canvas, and Trunk injects the generated wasm-bindgen loader. No handwritten JS application code is required.
- `eframe 0.37` uses `App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)` as the required method, with `logic(...)` for non-UI updates. Many older examples use `App::update`; we must follow the 0.37 docs, not old tutorials.

But there are real risks:

- **Tooling is missing:** Trunk, wasm-bindgen CLI, and wasm-pack are not installed. Trunk can fetch wasm-bindgen, but versions must align with `Cargo.lock`.
- **Browser matrix is undefined:** WebGPU is not universal. WebGL2 fallback should work for a 2D egui UI, but we should explicitly support/test both paths and define minimum browser versions.
- **WASM platform differences:** no threads, no `std::fs`, no native printing, different RNG/time/clipboard/file-dialog behavior. Core code must avoid platform APIs and use injected `Clock`, ID generator, and storage interfaces.
- **Binary size/startup:** `wgpu` on WASM produces a large `.wasm`. Release profile tuning, `wasm-opt`, and load-time measurement are needed.
- **Accessibility:** egui uses AccessKit; web screen-reader behavior is not equivalent to DOM accessibility. UI-UX requires keyboard navigation, visible focus, and labels, so this needs explicit verification and documented limitations.
- **Native Linux build dependencies:** the current machine has X11, Wayland, Vulkan, and GTK3 development libraries, but CI must install equivalent packages and provide a software GPU or use build-only checks.

**Recommendation:** pin `eframe = 0.37.0`, `egui = 0.37.0`, `wgpu` transitively at 30.x, and `wasm-bindgen` at 0.2.130. Set `Renderer::Wgpu` explicitly on both native and web so we do not accidentally use the `glow` renderer. Treat the first scaffold as the build proof.

### 3.3 Feature parity

Parity is achievable for business behavior if the architecture is:

- `openrs-core`: pure Rust domain logic, no UI or platform dependencies.
- `openrs-ui`: shared egui screens/widgets, depends on `egui` only.
- `openrs-desktop` and `openrs-web`: thin shells that only handle platform startup.

Parity testing proposal:

1. Run the **same core test suite natively** with `cargo test`.
2. Run the **same core tests in Chromium** with `wasm-bindgen-test` / `wasm-pack test --headless --chrome`. Chromium and ChromeDriver are already installed.
3. Run shared UI interaction tests natively with `egui_kittest` (AccessKit-based, no pixel snapshots initially).
4. Add Rust WebDriver browser tests later (`fantoccini` or `thirtyfour`) for end-to-end workflows, avoiding JS/TS test application code.
5. Keep a documented platform-difference list for printing, file dialogs, clipboard, and barcode input.

Pixel-identical UI across platforms should **not** be a goal; identical business outcomes and accessible behavior should be.

### 3.4 Data, storage, and checkout — the biggest unresolved area

`ROADMAP.md` explicitly leaves the database open. That is correct, but several SPEC requirements cannot be implemented safely until it is decided:

- “A failed checkout must not leave a partial sale or an incorrect stock count.”
- “Retrying the same checkout must not record the sale or deduct stock twice.”
- “A sale must retain the prices used at checkout even if product prices change later.”

These require **transactions**, **unique constraints**, and an **idempotency key**.

Options:

| Option | Pros | Cons |
|---|---|---|
| **A. SQLite everywhere** — `rusqlite` native; `sqlite-wasm-rs` + OPFS VFS on web | Same SQL/schema/transactions; highest logical parity | WASM SQLite/OPFS is complex; may require workers, SharedArrayBuffer/COOP-COEP headers; no threads; larger binary; needs a spike |
| **B. SQLite native + IndexedDB web** behind one repository trait | Web deployment is simpler; no COOP/COEP requirement | Two storage implementations; different transaction semantics; more parity tests |
| **C. JSON file native + localStorage/IndexedDB web** | Simplest to start | Weak atomicity and idempotency; not suitable for sales/inventory |
| **D. Remote server API** | Central data and multi-user potential | Contradicts local-first initial scope; offline/sync is explicitly later |

**Recommendation:** do **not** choose a database in Phase 0. Use an in-memory repository behind a trait for tests, and run a small spike before Phase 1 persistence comparing **A** and **B** on transactions, OPFS/browser support, bundle size, and testability. I recommend spiking **A first** because one schema and one transaction model are the strongest parity story, but I would not commit to it without that spike.

Related data-model decisions still missing:

- **ID strategy:** UUIDv7/ULID vs integer IDs. For local-first and future sync, opaque UUIDv7 newtypes are the safer recommendation, but WASM RNG configuration must be verified.
- **Idempotency key:** generate one stable key per checkout attempt, persist it with the sale, and enforce uniqueness. A retry returns the existing sale instead of deducting stock again.
- **Money:** `Money { minor_units: i64, currency: CurrencyCode }` with checked arithmetic; no floats. Decide whether the first release supports one currency per sale or multiple currencies.
- **Sale status:** SPEC mentions a status but never defines values. Recommend persisted `Completed` only for the initial release, with `Voided` reserved for later; failed checkouts should create no sale.
- **Timestamps:** store UTC, display local time. Decide whether the UI needs a configurable timezone.
- **Sale snapshots:** store unit price and currency at sale time; consider snapshotting product/variant name and SKU too, so receipts remain historically accurate.
- **Tax/discount/rounding:** explicitly deferred. Keep fields absent or zero until rules are approved.

### 3.5 Product scope and UX decisions missing

These are not implementation details; they change the data model and UI:

- **Product management:** SPEC only requires product *listing*. It does not specify product/variant creation, editing, deletion, CSV import, or stock receiving. Without any of these, the app is only usable with seeded data.
- **Stock replenishment:** stock only decreases unless there is a receiving/adjustment workflow. Phase 5 mentions restocking tests, but no UI scope.
- **Barcode input:** “if used” and “as supported” are ambiguous. Keyboard-wedge USB scanners can be treated as text input; camera scanning is a much larger platform-specific feature.
- **Product images:** optional in SPEC; storage and file-picker behavior differ on web and desktop.
- **Categories:** flat string vs hierarchy is undefined.
- **Variants:** attribute schema is undefined; key-value attributes with uniqueness rules are the likely minimum.
- **Search:** fields and matching rules are undefined (case-insensitive substring, SKU exact, barcode prefix, etc.).
- **Roles/authentication:** UI-UX lists this as open. The initial release could be single-user, no login, but that must be an explicit decision.
- **Payment:** SPEC excludes payment-provider integration. Decide whether a sale records a payment method (cash/card) or no payment information at all.
- **Receipt format and printing:** “printable preview” and “agreed format” are undefined. Options include PDF, HTML, plain text, and 80mm thermal/ESC-POS. Native printing is not provided by egui; a shared Rust document generator plus platform-specific handoff is likely needed.
- **Support matrix:** desktop OSes, browser versions, minimum window size, touch vs keyboard/mouse, dark theme, and accessibility target are all undecided.

### 3.6 Rust-only / no-webview compliance

The proposed stack complies **if**:

- We use `eframe`/`egui` with the `wgpu` renderer only, not `glow`.
- We do not add Tauri, Electron, a webview shell, or a JS/TS UI framework.
- The web app uses Trunk + wasm-bindgen; generated JS glue is allowed.
- `index.html` remains static HTML/CSS with no handwritten application JavaScript.
- Browser APIs needed for export/print are called from Rust via `web-sys`, not via handwritten JS.

One caveat: an offline PWA service worker is handwritten JavaScript. That is later scope; if offline web support is required, it needs an explicit exception or a generated alternative.

---

## 4. Small Phase 0 setup proposal

### Workspace structure

```text
OpenRS/
├── Cargo.toml                 # workspace, resolver 3, shared dependencies
├── rust-toolchain.toml        # pin 1.95.0 + wasm32 target + rustfmt/clippy
├── rustfmt.toml               # minimal, defaults initially
├── README.md                  # build/run/test instructions
├── AGENTS.md
├── SPEC.md
├── ROADMAP.md
├── UI-UX.md
├── docs/
│   └── adr/
│       └── 0001-workspace-and-targets.md
├── crates/
│   ├── openrs-core/           # pure domain logic, no UI/platform deps
│   ├── openrs-ui/             # shared egui screens/widgets
│   ├── openrs-desktop/        # eframe native shell
│   └── openrs-web/            # eframe WebRunner shell + index.html
└── .github/
    └── workflows/
        └── ci.yml
```

Key design choices:

- `openrs-core` has **no** `egui`, `eframe`, `std::fs`, or platform-specific dependencies.
- `openrs-ui` depends on `egui` only, so it can be tested with `egui_kittest` without a window.
- `openrs-desktop` and `openrs-web` are thin shells implementing `eframe::App::ui` by delegating to `openrs-ui`.
- `openrs-web/index.html` uses Trunk’s `data-trunk rel="rust"`; no handwritten JS.
- Phase 0 uses **no database** and **no persistence**. Storage is deferred.

### Checks

Native:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

WASM:

```bash
cargo check -p openrs-web --target wasm32-unknown-unknown
trunk build --release crates/openrs-web/index.html
wasm-pack test --headless --chrome crates/openrs-core
```

Optional quality gates:

```bash
cargo deny check
```

CI should run native and WASM jobs. The native job needs X11/Wayland/Vulkan development packages; the WASM job needs Trunk, wasm-pack, Chromium, and ChromeDriver. The desktop smoke test can use `xvfb-run` with a software renderer if needed.

### Smallest useful first milestone

**Milestone 0.1 — “Hello OpenRS” on desktop and web.**

- Create the workspace and four crates.
- Add one shared, tested pure function in `openrs-core` (no retail logic yet).
- Render “OpenRS” plus that shared value in the shared `openrs-ui` screen.
- Run it through the native eframe shell and the Trunk-built web shell.
- Prove that the same core test runs natively and in Chromium.
- Add README, ADR, and CI.
- Do not add a database, product model, or bill logic yet.

This validates the risky part of Phase 0 — the toolchain and parity path — before any product feature work.

---

## 5. Recommended dependencies

### Phase 0 required

| Crate | Version | Targets | Why |
|---|---:|---|---|
| `eframe` | 0.37.0 | native + wasm | Required app shell; enable `wgpu`, `default_fonts`, `accesskit`, `wayland`, `x11`; disable default features and never enable `glow` |
| `egui` | 0.37.0 | native + wasm | Shared UI crate; keep it separate from eframe |
| `thiserror` | 2.0 | native + wasm | Typed domain/validation errors without hand-written boilerplate |
| `log` | 0.4 | native + wasm | Shared logging facade |
| `wasm-bindgen-futures` | 0.4 | wasm only | Spawn the async `WebRunner` startup from Rust |
| `web-sys` | 0.3 | wasm only | Get the canvas and loading element; no JS application code |
| `console_error_panic_hook` | 0.1 | wasm only | Readable panic messages in the browser console |
| `egui_kittest` | 0.37.0 | native tests | Headless egui interaction/AccessKit tests |
| `wasm-bindgen-test` | 0.3 | wasm tests | Run shared core tests in a real browser |

### Recommended soon, but not Phase 0

| Crate | Version | Targets | Why / caveat |
|---|---:|---|---|
| `serde` | 1.0 | native + wasm | Persistence, export, fixtures |
| `uuid` | 1.29 | native + wasm | IDs; use v7 + `serde`; verify WASM RNG feature/cfg |
| `time` | 0.3 | native + wasm | UTC timestamps and formatting; enable `wasm-bindgen` |
| `proptest` | 1.12 | native tests | Property tests for money/bill arithmetic |
| `rusqlite` | 0.41 | native; wasm only after spike | Native SQLite; WASM requires `sqlite-wasm-rs` + OPFS VFS |
| `indexed_db_futures` | 0.6 | wasm only | If Option B (IndexedDB web) is chosen |
| `printpdf` or `typst` | current | native; wasm needs verification | Receipt PDF generation; evaluate size and WASM support |
| `directories` | 6.0 | native only | Platform data directory for the database |
| `rfd` | 0.17 | native only | File dialogs; defer until export/import work |

### Build/test tools, not application dependencies

- `trunk 0.22.1` — approved Rust-to-WASM bundler.
- `wasm-bindgen-cli` matching the `wasm-bindgen` crate (0.2.130).
- `wasm-pack 0.15.1` — browser test runner only, not the app bundler.
- `wasm-opt` (Binaryen) — release size optimization.
- `cargo-nextest` — optional faster native tests.
- `cargo-deny` — optional license/advisory checks.
- `just` — optional; it is not currently on `PATH`, so I would use direct Cargo commands or a Rust `xtask` later rather than relying on it.

### Do not add now

- Tauri, Electron, webviews, or any JS/TS UI framework.
- `glow` as a renderer; it would violate the wgpu requirement.
- A database/ORM before the storage decision and spike.
- `rust_decimal` while the SPEC requires integer minor units; add only if tax-rate math needs it.
- Both `chrono` and `time`; choose one.

---

## Decisions needed before Phase 1

1. **Repository/docs:** Should the root become the canonical Git repo with `AGENTS.md`, `SPEC.md`, `ROADMAP.md`, `UI-UX.md`, and `docs/` tracked, while `.LOCAL/` stays private? Recommended: yes.
2. **License:** MIT/Apache-2.0 dual license, AGPL-3.0, or another license? Do not choose this silently.
3. **Storage:** After a short spike, do you prefer SQLite-everywhere (one schema/transactions, more WASM complexity) or SQLite-native + IndexedDB-web (simpler deployment, two adapters)? Recommended: spike SQLite-everywhere first.
4. **Product scope:** Is the first release read-only catalog with seeded/imported data, or does it need minimal product/variant CRUD and stock receiving? Recommended: read-only/import first, full CRUD later, to keep the first sale flow small.
5. **Money/IDs:** Single currency per sale, UUIDv7 IDs, and a checkout idempotency key? Recommended: yes, unless multi-currency or integer IDs are required.
6. **Receipt:** PDF first, HTML first, or 80mm thermal first? Recommended: shared Rust-generated PDF/HTML, browser print on web, system viewer on desktop; thermal later.
7. **Support matrix:** Which desktop OSes and browser versions must work? Recommended: Linux/Windows/macOS desktop; latest two versions of Chrome/Edge/Firefox/Safari; WebGL2 required, WebGPU optional.
8. **UX defaults:** Single-user/no login, no payment provider, keyboard-wedge barcode input, no camera scanning, light/dark theme, minimum window around 1024×700. Confirm or change.

---

## Recommended next task

**Phase 0.1 — Scaffold the Rust workspace and prove the desktop + WebAssembly toolchain with a shared “Hello OpenRS” screen.**

Scope:

- Initialize the root Git repository and track the canonical docs.
- Add `rust-toolchain.toml` pinning Rust `1.95.0` and `wasm32-unknown-unknown`.
- Create `openrs-core`, `openrs-ui`, `openrs-desktop`, and `openrs-web`.
- Add one shared, tested pure function in `openrs-core`; no retail logic.
- Render it through the shared egui UI in both shells.
- Add a static Trunk `index.html` with no handwritten JavaScript.
- Add README, one ADR for the workspace/targets, and a CI workflow.
- Add no database, no persistence, and no product features.

Acceptance criteria:

- `cargo fmt --all --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes, including the shared core test.
- `cargo check -p openrs-web --target wasm32-unknown-unknown` passes.
- `trunk build --release crates/openrs-web/index.html` succeeds.
- `wasm-pack test --headless --chrome crates/openrs-core` passes the same core test in Chromium.
- The desktop binary launches and shows “OpenRS” plus the shared value.
- The Trunk-served web build shows the same value in Chromium.
- The repository contains no Tauri/Electron/webview dependency and no handwritten JavaScript application code.
- README documents the exact build/run/test commands, and the ADR records the target matrix and open decisions.
- CI runs the native and WASM checks.

**Approval status:** waiting for project-owner approval before editing code or adding dependencies.
