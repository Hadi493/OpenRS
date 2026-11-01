# ADR 0001: Workspace and platform targets

- **Status:** Accepted for Phase 0.1
- **Date:** 2026-11-02

## Context

OpenRS must ship as a native desktop application and as a WebAssembly web
application while keeping business behavior consistent. The project rules
forbid Tauri, Electron, webview shells, JavaScript/TypeScript application code,
and JavaScript UI frameworks.

The desktop UI must use `egui`/`eframe` with `wgpu`. The web build must use the
approved Rust-to-WASM path: Trunk + `wasm-bindgen`.

## Options considered

1. **Single crate with `cfg(target_arch = "wasm32")` branches.** Small initial
   tree, but platform-specific dependencies and startup code become entangled
   with domain logic.
2. **Separate shell crates plus shared core/UI crates.** Slightly more files,
   but clear dependency boundaries and testable shared behavior.
3. **Tauri/Electron/webview shell.** Rejected: explicitly forbidden and would
   introduce a webview and JavaScript application layer.

## Decision

Use a Cargo workspace with four crates:

- `openrs-core`: pure Rust domain logic. No `egui`, `eframe`, filesystem, or
  other platform services.
- `openrs-ui`: shared `egui` UI. Depends on `egui` only, not `eframe`.
- `openrs-desktop`: native `eframe` + `wgpu` shell.
- `openrs-web`: `eframe::WebRunner` shell built by Trunk.

The shells are intentionally thin: they own window/canvas startup and delegate
rendering to `openrs-ui`. This keeps the UI testable with `egui_kittest` and
makes the native/web behavior boundary explicit.

Pin `eframe`, `egui`, and `egui_kittest` to `0.37.0` while that API is new.
Build with at most two parallel Cargo jobs, as required by the project owner.

## Consequences

- Shared logic can be tested natively and in a browser with the same contract
  tests.
- The web build requires Trunk and a wasm-bindgen version aligned with
  `Cargo.lock`.
- Platform-specific behavior (printing, file dialogs, clipboard, storage) must
  be isolated behind adapters and documented.
- No database or persistence is introduced in Phase 0.1. That decision is
  deferred until a storage spike and project-owner approval.

## Open decisions

- Database/storage engine and transaction strategy
- Product-management scope
- Receipt format and printing
- Currency, IDs, and sale status
- Supported OS/browser/accessibility matrix
- License
