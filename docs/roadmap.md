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

## Later milestones

- Evaluate additional detector rules against representative fixtures and measure artifact detection overhead on large trees.
- Design a separate inventory for global package caches.
- Design an explicit review and confirmation flow for cleanup, including exclusions and an assessment of recoverable removal options.
- Validate the Windows build on a clean machine and settle licensing before distributing it. Add packaging and signing, then validate the core and GUI on Linux and macOS.
