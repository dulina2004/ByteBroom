# Roadmap

## Application shell

- Dark native Slint desktop window with Dashboard, Scan, Cleanup, and Settings.
- Windows drive discovery with volume metadata, usage bars, and on-demand refresh.
- Cargo validation commands and Windows release build instructions.

## Read-only drive and folder scan

- Start a read-only scan from a drive card or the Windows folder picker.
- Enumerate off the UI thread with bounded memory, live progress, and cancellation.
- Report logical sizes, inaccessible entries, skipped reparse points, and the largest immediate subdirectories.

## Developer artifact detection

- Identify supported projects and generated directories during read-only scans.
- Browse logical sizes, ownership, confidence, and regeneration guidance by artifact or project.

## Global developer storage

- Implemented a two-stage Windows inventory for supported package managers, Android resources, editors, testing tools, and ML caches.
- Implemented selection by location, scoped measurement, cancellation, and paginated browsing with risk and measurement details.
- Provider discovery covers documented defaults, supported environment overrides, and session-only custom locations. Configuration files, portable layouts, and some version-specific paths remain open. [Provider coverage](development-storage.md)

## Storage Analyzer

- Implemented bounded directory exploration, native treemaps, filtered analysis views, and comparisons for two compatible in-memory directory scans.
- Added project, artifact, ecosystem, and global resource views. File Explorer integration, report export, and more advanced filters are deferred. [Analyzer guide](storage-analyzer.md)

## Cleanup planning

- Implemented explicit candidate selection, session exclusions, bounded Windows identity checks, and a cancellable read-only dry run.
- Verified single-use authorization and revalidation with a test-only recording backend. Recycle Bin, permanent removal, recovery reporting, and cleanup history remain deferred pending an execution safety review. [Cleanup safety](cleanup-safety.md)

## Later milestones

- Evaluate additional detector rules against representative fixtures and measure artifact detection overhead on large trees.
- Design protected, host-only Docker and WSL storage reporting.
- Evaluate allocated-size reporting and combined project/global accounting. Benchmark low-memory and cancellation behavior with synthetic large caches.
- Validate the Windows build on a clean machine and settle licensing before distributing it. Add packaging and signing, then validate the core and GUI on Linux and macOS.
