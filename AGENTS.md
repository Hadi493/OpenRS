# AGENTS.md: guide for AI agents and contributors

**OpenRS** is an open-source, native, high quality retail software comparable Modern Retail Software written in **Rust-only** (no Javascript or typescript) **No Tauri, Electron or webview Shells:** the desktop app is native egui/eframe on wgpu, and the web build is the same Rust compiled to WebAssembly (trunk + wasm-bindgen). Never add Tauri (or any webview/JS UI framework) as a dependency, build step or packaging target. The product name is always written **OpenRS** (`{Function}OpenRS` in PascalCase.

## Rules & Notes:
- I want to develop the software slow and in test driven way.
  - Every feature should test in multiple ways and find the best algorithm for that feature.
- Every function/feature should have its note in detailed.
- *If any feature is debatable or difficult to implement hand over to me with its details*
*BTW I love Programming and problem solving*

## Development Principles

- Build OpenRS incrementally. Prefer small, reviewable changes over large batches of work.
- Use test-driven development where practical: write or update a failing test, implement the smallest suitable change, then run the relevant checks.
- Before implementing a feature, outline its purpose, expected behavior, edge cases, and test plan.
- For nontrivial features, compare plausible approaches and explain why the chosen approach is suitable. Avoid adding abstractions or dependencies without a clear need.
- Do not silently make product, UX, data-model, or architectural decisions that are difficult to reverse.

## Rust and Platform Constraints

- Application and business logic must be written in Rust.
- Do not add handwritten JavaScript or TypeScript, JavaScript-based UI frameworks, webview shells, Tauri, or Electron.
- Web builds must use the existing Rust-to-WebAssembly approach. Generated JavaScript glue or build artifacts are acceptable only when produced by the approved Rust/WASM toolchain; do not write or maintain JavaScript application code.
- Keep desktop and web behavior as consistent as the platforms allow. Document any unavoidable platform-specific behavior.
- Before adding a dependency, explain why it is needed and check that it respects the project’s Rust-only and platform constraints.

## Testing and Quality

- Test behavior at the appropriate levels: unit tests for isolated logic, integration tests for component interactions, and UI or end-to-end tests when practical.
- Include boundary cases, invalid inputs, and failure paths—not only the happy path.
- Do not claim a feature is complete until the relevant tests and project checks have been run. Report any checks that could not be run and why.
- Prefer deterministic tests. Avoid relying on network access, real payment services, or production data in tests.
- For performance-sensitive or algorithmic work, define the workload and success criteria before benchmarking. Compare alternatives only when the difference matters to the feature.

## Documentation

- Add or update user-facing and developer documentation when behavior, setup, configuration, or architecture changes.
- Document public APIs and non-obvious design decisions. Comments should explain intent, constraints, and surprising behavior—not merely repeat what the code does.
- For significant architectural decisions, add a short decision record describing the context, options considered, decision, and consequences.

## When to Ask the Project Owner

Pause and ask before proceeding when:
- Requirements are ambiguous in a way that could change user-visible behavior or data.
- A feature has significant security, privacy, payment, compliance, or migration implications.
- The implementation requires a new architectural direction or a dependency that might conflict with project constraints.
- The available approaches have meaningful tradeoffs and no clear default.

When asking, provide the open questions, viable options, tradeoffs, and a recommended option. Continue with independent, low-risk work where possible.

## Platform Parity

- OpenRS supports both the native desktop app and the WebAssembly web app.
- Every user-facing feature in the agreed release scope must work on both targets.
- Business rules and data validation must live in shared Rust code wherever practical.
- Platform-specific UI or operating-system behavior is allowed, but must not create
  different business outcomes or silently missing functionality.
- Each requirement must state which targets it applies to and how parity is tested.
- Do not mark a feature complete until its acceptance tests pass on every supported
  target, or the project owner has approved a documented exception.
- Track approved differences, their reasons, and whether they are temporary or
  permanent.


 > NOTE: compile only with 2 theats ```--jobs 2```
