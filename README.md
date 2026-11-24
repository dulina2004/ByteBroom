# ByteBroom

**ByteBroom — A fast, lightweight, open-source disk analyzer and cleanup tool for developers, built with Rust and Slint.**

ByteBroom aims to help developers understand how dependencies, build artifacts, and package caches use their storage. The first target is Windows 11. Linux and macOS support are planned.

## Current status

The dashboard shows the drive letters visible to the current Windows session, with volume labels when available, drive types, capacities, and usage. Select **Refresh** to update the snapshot. Totals reflect the listed drive letters. Capacities use binary units such as GiB and TiB.

Select **Scan** on a drive card or **Scan a folder…** to start a read-only metadata scan. The Scan page shows live counts and elapsed time, then lists up to twelve of the largest immediate subdirectories. Select **Cancel Scan** to stop and keep partial results. The scanner reports inaccessible entries and skips symbolic links, junctions, mount points, and other reparse points.

Scan totals sum logical file lengths, which can differ from allocated disk space. Each hard-linked name counts as a separate entry. Results reflect the metadata seen during the scan as the filesystem changes.

The desktop shell uses Rust 2024 and Slint 1.19.0 with the Winit backend and Slint's native software renderer. Cargo compiles the Slint files during the build.

## Windows development

- Use [stable Rust and Cargo via rustup](https://rustup.rs/) with Rust 1.95 or newer and the `x86_64-pc-windows-msvc` target.
- In Visual Studio Installer, select **Desktop development with C++** with the x64 MSVC toolset and a Windows SDK. Rust uses the MSVC linker and the SDK import libraries. [Microsoft's Rust setup guide](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
- Git is useful for contributing. The Rust build requires Cargo and the Windows tools above.

In a PowerShell window opened after toolchain setup, run:

```powershell
cargo build --locked
cargo run --locked
```

If Cargo is installed and PowerShell cannot find it, add the rustup bin directory for that session with `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`. If a build reports `link.exe` missing, use Visual Studio Installer to add the C++ workload and reopen the terminal.

In the app, use the sidebar or Tab and Enter to move through the four pages. For a quick smoke test, select **Scan a folder…** and choose a small fixture with known file sizes.

To build a Windows release executable, run `cargo build --release --locked`. The output is `target/release/bytebroom.exe`. For distribution, check runtime requirements on a clean Windows VM before adding installer metadata, signing, or an MSIX package. [Slint Windows packaging guide](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/packaging/windows/)

## Contributing

Read [Repository Guidelines](AGENTS.md) and [Architecture](docs/architecture.md). Use focused pull requests, describe what you verified, and add screenshots for visual changes. Keep `Cargo.lock` in version control. See the [Roadmap](docs/roadmap.md) for upcoming work.

```powershell
cargo fmt --all -- --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Licensing

ByteBroom's project license is pending. Before distributing builds, select ByteBroom's license and a compatible Slint licensing option. Slint offers GPLv3 for GPL-compatible open-source distribution, a royalty-free option with Slint attribution requirements for desktop applications, and a commercial option. Review the complete terms, including the required attribution if choosing the royalty-free option. [Slint licensing](https://github.com/slint-ui/slint/blob/v1.19.0/LICENSE.md)
