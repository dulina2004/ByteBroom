# Architecture

ByteBroom starts as a single Cargo package, `bytebroom`. Its only direct dependencies are Slint at runtime and `slint-build` at compile time.

## Current structure

| Location                                        | Responsibility                                                                                       |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `src/main.rs`                                   | Starts the app and returns startup errors. Windows release builds use the windowed subsystem.        |
| `src/app.rs`                                    | Creates the window, maps Rust values to Slint view data, and starts the UI event loop.               |
| `src/models.rs`                                 | Defines sample drive capacities and tests their arithmetic. Depends only on Rust's standard library. |
| `build.rs`                                      | Compiles `ui/app-window.slint` with Slint's dark Fluent style for standard widgets.                  |
| `ui/app-window.slint`                           | Owns the active page and the shared header.                                                          |
| `ui/components/`, `ui/pages/`, `ui/theme.slint` | Hold reusable controls, page layouts, and theme colors.                                              |

At startup, the Rust model feeds a small Slint model and the aggregate display values. Navigation stays within the UI. The Slint Winit backend opens a native window and the software renderer draws the interface. The shell has no timers, filesystem workers, or persistent settings.

## Boundaries for future work

Add the scanner only when implementing read-only enumeration. It should accept an explicit root and options, expose progress and cancellation in Rust types, and remain independent of Slint. A future CLI should be able to call the same scanner without creating a window. Start with standard Rust filesystem APIs, then isolate platform-specific behavior such as Windows reparse point handling in dedicated modules.

Run enumeration off the UI thread. Use bounded work and result queues, cap retained results, and coalesce progress updates before sending them to Slint's event loop. Add detectors when there are classification rules and fixture tests to support them. Keep cleanup policy separate from enumeration and evaluate it after read-only scanning has been validated.
