# Architecture

ByteBroom is a single Cargo package, `bytebroom`. It uses Slint at runtime, `slint-build` at compile time, and `windows-sys` for Windows volume metadata, scanning, and folder selection.

## Current structure

| Location                                        | Responsibility                                                                                       |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `src/main.rs`                                   | Starts the app and returns startup errors. Windows release builds use the windowed subsystem.        |
| `src/app.rs`                                    | Starts Slint and coordinates drive refreshes and presentation.                                       |
| `src/storage/mod.rs`                            | Defines drive snapshots, capacity arithmetic, and formatting independently of Slint.                  |
| `src/storage/windows.rs`                        | Queries drive letters, types, labels, and capacity from Win32.                                       |
| `src/scanner/`                                 | Traverses Windows directories and returns plain Rust progress, statistics, and results.               |
| `src/app/scan.rs` and `src/app/folder_picker.rs` | Coordinate one scan or folder picker at a time and present the results in Slint.                      |
| `build.rs`                                      | Compiles `ui/app-window.slint` with Slint's dark Fluent style for standard widgets.                  |
| `ui/app-window.slint`                           | Owns the active page and the shared header.                                                          |
| `ui/components/`, `ui/pages/`, `ui/theme.slint` | Hold reusable controls, page layouts, and theme colors.                                              |

A single background worker discovers drives on startup and on demand. It returns plain Rust data through Slint's weak-handle event-loop dispatch. Slint models are created and replaced on the UI thread. Refresh is disabled while discovery runs. When one drive is unavailable, the dashboard shows the remaining drives and reports the skipped letter.

The Windows adapter queries drive roots and suppresses Windows critical-error dialogs on the worker thread. It uses volume-wide capacity where available. Its fallback pairs the current user's total and free bytes so the figures remain consistent on quota-managed drives. The dashboard aggregates listed drive letters, so multiple letters that point to the same storage can count that capacity more than once.

## Read-only filesystem scanning

The scanner uses a depth-first stack of open directories with one fixed-size metadata buffer per active depth. It counts logical lengths from directory records, retains at most twelve immediate-child summaries, and never opens file contents. The Windows adapter opens each child by name relative to a validated parent handle and checks the opened handle for reparse points before listing it. It applies those checks to the selected folder's ancestors too. Unavailable child entries are recoverable, while failure to open or read the scan root ends the scan.

A background worker replaces a single progress snapshot at most about eight times per second. A Slint timer polls that snapshot through weak handles and joins the worker after it exits. Cancellation sets an atomic flag and asks Windows to cancel pending synchronous I/O. Providers that cannot interrupt a request can delay the final cancelled state. Closing the application during a scan requests cancellation and keeps the window responsive until the worker stops.

The native folder picker runs on a dedicated Windows STA thread and uses the selection-only tree style. The app retains the selected path in memory for the duration of the UI session.

A future CLI can call the same scanner without opening a window. Keep artifact detection and cleanup policy separate from enumeration.
