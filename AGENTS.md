# Repository Guidelines

## Project Structure

- `src/main.rs` starts the Windows-first desktop application. `src/app.rs` owns Slint initialization and maps storage results to UI data. Keep `src/storage/` independent of Slint so future scanner and CLI code can reuse it.
- `ui/app-window.slint` owns window navigation. Reuse the theme and controls in `ui/theme.slint` and `ui/components/`. Keep each page in `ui/pages/`.
- See [Architecture](docs/architecture.md) for module boundaries and [Roadmap](docs/roadmap.md) for scope.

## Rust and Slint

Prefer idiomatic Rust, small functions with explicit inputs, actionable errors with context, and the standard library where practical. Use Rustfmt, descriptive snake_case Rust names, and kebab-case Slint properties. Document public contracts and non-obvious invariants. Ask before adding major dependencies.

Keep navigation and transient presentation state in Slint. Keep filesystem and application decisions in Rust. Use shared colors and components, accessible labels, visible focus, and keyboard activation. Update UI models only for meaningful changes.

## Performance and Filesystem Safety

Keep filesystem work off the UI thread. Process large trees incrementally with bounded concurrency and result batches. Avoid unnecessary copying and allocations, keep limits explicit, and design long-running work for cancellation. Measure CPU, memory, throughput, and UI latency before optimizing.

Keep scanning read-only. Treat input paths as untrusted. Skip symlinks and Windows junctions by default, and surface permission failures gracefully. Folder names alone never establish deletion safety. Never automatically request administrator privileges. Obtain explicit approval for destructive operations and before adding destructive behavior. Use generated fixtures in temporary test directories for filesystem tests and never delete actual user files.

## Verification and Reviews

Inspect the tree before editing. For substantial work, outline a short plan, implement incrementally, and keep changes focused. Identify risks before changing filesystem code. Explain architectural decisions and update docs with the implementation.

Co-locate Rust unit tests and add integration tests in `tests/` when testing public workflows. Test behavior and edge cases. Run `cargo fmt --all -- --check`, `cargo check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Launch with `cargo run` for UI changes and report any checks blocked by the environment. Track `Cargo.lock`.

Use imperative commit subjects. In pull requests, describe the change and verification, link related issues, and include screenshots for visual changes.
