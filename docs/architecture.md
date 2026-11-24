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
| `src/global_caches/`                           | Resolves known developer storage, validates local paths, and schedules selected measurement roots.   |
| `src/analysis/`                                | Builds disjoint analysis views, bounded hierarchy snapshots, comparisons, and treemap geometry.      |
| `src/app/analyzer.rs`                           | Maps analysis snapshots to paginated Slint models and coalesces search and resize events.             |
| `src/cleanup/`                                 | Derives eligible candidates, enforces exclusions, and produces immutable read-only plans.            |
| `src/app/cleanup.rs`                            | Presents the Cleanup selection, validation progress, and preview in Slint.                           |
| `src/app/development.rs`                       | Presents the inventory, selection, filters, and paginated results in Slint.                          |
| `src/app/scan.rs` and `src/app/folder_picker.rs` | Coordinate one scan or folder picker at a time and present the results in Slint.                      |
| `build.rs`                                      | Compiles `ui/app-window.slint` with Slint's dark Fluent style for standard widgets.                  |
| `ui/app-window.slint`                           | Owns the active page and the shared header.                                                          |
| `ui/components/`, `ui/pages/`, `ui/theme.slint` | Hold reusable controls, page layouts, and theme colors.                                              |

A single background worker discovers drives on startup and on demand. It returns plain Rust data through Slint's weak-handle event-loop dispatch. Slint models are created and replaced on the UI thread. Refresh is disabled while discovery runs. When one drive is unavailable, the dashboard shows the remaining drives and reports the skipped letter.

The Windows adapter queries drive roots and suppresses Windows critical-error dialogs on the worker thread. It uses volume-wide capacity where available. Its fallback pairs the current user's total and free bytes so the figures remain consistent on quota-managed drives. The dashboard aggregates listed drive letters, so multiple letters that point to the same storage can count that capacity more than once.

## Read-only filesystem scanning

The scanner uses a depth-first stack of open directories with one fixed-size metadata buffer per active depth. It counts logical lengths from directory records and retains bounded directory summaries. The Windows adapter accepts direct local drive mappings and opens each child by name relative to a validated parent handle. It checks opened handles for reparse points, including the selected folder's ancestors. Unavailable child entries are recoverable, while failure to open or read the scan root ends the scan.

A background worker replaces a single progress snapshot at most about eight times per second. A Slint timer polls that snapshot through weak handles and joins the worker after it exits. Cancellation sets an atomic flag and asks Windows to cancel pending synchronous I/O. Providers that cannot interrupt a request can delay the final cancelled state. Closing the application during a scan requests cancellation and keeps the window responsive until the worker stops.

The native folder picker runs on a dedicated Windows STA thread and uses the selection-only tree style. The app retains the selected path in memory for the duration of the UI session.

A future CLI can call the same scanner without opening a window.

## Developer artifact detection

`src/detectors/` matches project and output markers against directory records. The scanner folds child sizes, counts, and access failures into the parent as each directory closes. Artifact measurement reuses those totals and issues no separate traversal. Files contribute their logical lengths and marker bits. Individual file paths are discarded.

The detector retains at most 12,000 relevant directory observations within an estimated 16 MiB observation budget, with capacity reserved for project markers. After traversal, it resolves records in parent-first order. A project can supply more than one ecosystem. Rules select immediate, grandparent, or nearest relevant project ownership. A classified artifact owns its whole measured subtree, so descendant artifacts and projects are excluded from the artifact totals. Known dependency and protected trees skip project detection while still contributing to general scan totals.

`src/app/artifacts.rs` builds search and sort indexes on the scan worker. The UI filters the retained records in memory and renders 12 rows per page. Cancelling, encountering access errors, or exceeding an observation limit marks results partial.

To add a detector, register contextual markers in `src/detectors/rules.rs`, choose an ownership relationship, category, confidence, and review classification, and add positive and false-positive fixtures under `src/scanner/tests/`. Use metadata already available during traversal and keep each rule independent of Slint.

## Global developer storage

The Development Storage workflow separates discovery from measurement. Static providers build typed location plans from injected known folders, a fixed environment-variable allowlist, and session-only custom roots. Discovery verifies expected directories or a bounded set of well-known child layouts. It caps the preview at 512 locations, inspects up to 1,024 immediate entries per layout, and shows notices for invalid settings and incomplete discovery.

The Windows adapter resolves the current user's known folders and checks drive-letter device mappings before probing paths. The scanner opens directory components relative to their parent handles and rejects reparse points. Discovery identifies duplicate roots using the volume serial number and file ID, and retains a normalized volume path for ancestry checks. Measurement verifies the directory still matches its discovery identity before reusing the opened handle. [Windows file identity](https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_id_info) and [normalized handle paths](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew)

Each discovered descendant has its own boundary. The shared walker skips separately listed descendants when measuring a parent, even when the descendant is unchecked. Each selected row contributes its own logical total once. The global worker uses the same iterative scanner without project detection. Android snapshot and virtual disk summaries come from metadata in that same pass. Hard-linked file names are counted individually. Project and global scans keep their own totals. A combined cross-scan total would require additional overlap reconciliation.

The shared scan controller runs one folder picker, project scan, discovery, or measurement job at a time. Worker threads replace a progress snapshot and the Slint timer polls it. Storage scan IDs guard against stale updates. Preview scope shows selected drives before measurement. Rust owns the in-memory inventory and filters, and Slint displays up to ten rows per page. Risk, confidence, and measurement status are separate fields.

See the [provider guide](development-storage.md) for adding location rules and fixtures.

## Storage Analyzer

Each project scan collects directory summaries in the existing traversal. It retains the largest twelve children of the root and six children per retained folder for the next two levels. This caps each published hierarchy at 517 nodes. Relative names share their parent context, and full paths are built from the retained route as needed. Each expanded node has disjoint child, direct-file, and omitted-child buckets. Unavailable entries affect completeness and contribute only the bytes actually observed.

The scan worker builds project, artifact, and ecosystem projections from the detector results. Each artifact has one owning project and one ecosystem for byte accounting. Multi-ecosystem projects may appear in more than one ecosystem's project count. The global scan worker builds separate resource, tool, ecosystem, drive, category, and risk projections. Project and global measurements can overlap and remain separate from each other and from Windows capacity.

The UI retains two project scan snapshots and the current global analysis. It browses up to 512 rows per view, renders twelve rows per page, and debounces search and resize. Rust computes the treemap by deterministic balanced binary splits along the longest axis. At most 80 tiles reach Slint. Smaller positive-size rows are grouped for chart display, and zero or unknown sizes stay in the list. Geometry uses viewport-relative coordinates and is recalculated after a resize settles.

Comparison checks the verified volume, directory ID, and normalized root path. Only complete scans are eligible. Root totals cover all measured data. If either snapshot omits children, per-directory comparison uses the intersection of both retained lists. Scan snapshots remain in memory, and local export is planned.

## Cleanup planning

The existing scanner records a bounded set of project and relevant target identities while it still holds directory handles. Cleanup derives a separate, bounded candidate registry from the latest scan. Slint passes candidate IDs. Rust retains paths, ownership, measurements, and evidence and remains responsible for eligibility.

A preview request snapshots the selection, exclusions, scope, and session, scan, and revision identifiers. A worker uses read-only handle-relative directory enumeration to verify the scope, project marker, target identity, protected-content policy, and measured counts. Canonical component ancestry and filesystem IDs reject overlapping operations. Slint polls a single progress snapshot through the shared scan controller, which runs one preview, scan, or folder picker at a time. User cancellation preserves already validated results and stops further checking.

The preview is immutable after publication. Changing selections or exclusions, closing the preview, or starting a newer scan invalidates it. A recording backend compiled only into tests exercises single-use authorization, method binding, revalidation, and cancellation. Production provides preview only. Execution results, space recovery, and cleanup history depend on a reviewed removal backend. [Cleanup safety](cleanup-safety.md)
