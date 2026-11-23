#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod models;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
