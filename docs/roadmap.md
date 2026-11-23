# Roadmap

## Application shell

- Dark native Slint desktop window with Dashboard, Scan, Cleanup, and Settings.
- Example drives and a segmented storage usage display with clearly labeled mock data.
- Cargo validation commands and Windows release build instructions.

## Next milestone — Read-only folder scan

- Let users choose a single directory and confirm the scan scope.
- Enumerate off the UI thread with bounded memory, progress, and cancellation.
- Report accessible sizes and skipped entries. Skip symlinks and junctions by default, and test permission and cancellation behavior with generated fixtures.
- Measure throughput, memory, and UI responsiveness before adding a full-drive scan.

## Later milestones

- Identify common developer artifacts using evidence from project metadata and filesystem context. Display the reason for each classification and its size.
- Design an explicit review and confirmation flow for cleanup, including exclusions and an assessment of recoverable removal options.
- Validate the Windows build on a clean machine and settle licensing before distributing it. Add packaging and signing, then validate the core and GUI on Linux and macOS.
