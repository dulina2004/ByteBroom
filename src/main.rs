#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod analysis;
mod app;
mod cleanup;
mod detectors;
#[cfg(windows)]
mod global_caches;
mod scanner;
mod storage;
#[cfg(windows)]
mod windows_fs;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
