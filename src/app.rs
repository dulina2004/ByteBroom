use std::{io, rc::Rc, thread};

use slint::{ComponentHandle, PlatformError, VecModel};

use crate::storage::{self, Capacity, Discovery, Drive, format_bytes};

mod analyzer;
mod artifacts;
mod cleanup;
#[cfg(windows)]
mod development;
#[cfg(windows)]
mod folder_picker;
mod scan;

slint::include_modules!();

pub(crate) fn run() -> Result<(), PlatformError> {
    let window = AppWindow::new()?;
    window.set_app_version(env!("CARGO_PKG_VERSION").into());

    let weak = window.as_weak();
    window.on_refresh_drives(move || {
        if let Some(window) = weak.upgrade() {
            refresh_drives(&window);
        }
    });
    refresh_drives(&window);

    let scans = scan::ScanController::attach(&window);
    let result = window.run();
    scans.shutdown();
    result
}

fn refresh_drives(window: &AppWindow) {
    if window.get_loading_drives() {
        return;
    }
    window.set_loading_drives(true);
    window.set_drive_error("".into());
    window.set_drive_notice("".into());

    let weak = window.as_weak();
    let worker = thread::Builder::new()
        .name("drive-discovery".into())
        .spawn(move || {
            let result = storage::discover();
            // The window may close while the metadata query is in progress.
            let _ = weak.upgrade_in_event_loop(move |window| finish_refresh(&window, result));
        });
    if let Err(error) = worker {
        finish_refresh(window, Err(error));
    }
}

fn finish_refresh(window: &AppWindow, result: io::Result<Discovery>) {
    let mut error = String::new();
    let mut notice = String::new();
    let snapshot = match result {
        Ok(snapshot) => {
            if !snapshot.unavailable.is_empty() {
                let letters = snapshot
                    .unavailable
                    .iter()
                    .map(|letter| format!("{letter}:"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let unavailable = format!("Capacity is unavailable for {letters}.");
                if snapshot.drives.is_empty() {
                    error = unavailable;
                } else {
                    notice = unavailable;
                }
            }
            snapshot
        }
        Err(cause) => {
            error = cause.to_string();
            Discovery::default()
        }
    };

    let totals = snapshot.total_capacity();
    window.set_total_space(format_total(totals, Capacity::total_bytes).into());
    window.set_used_space(format_total(totals, Capacity::used_bytes).into());
    window.set_free_space(format_total(totals, Capacity::free_bytes).into());
    let cards = snapshot.drives.iter().map(drive_card).collect::<Vec<_>>();
    window.set_drives(Rc::new(VecModel::from(cards)).into());
    window.set_drive_error(error.into());
    window.set_drive_notice(notice.into());
    window.set_loading_drives(false);
}

fn drive_card(drive: &Drive) -> DriveCardData {
    let usage = drive.capacity.used_fraction();
    DriveCardData {
        letter: format!("{}:", drive.letter).into(),
        name: drive.label.as_str().into(),
        kind: drive.kind.into(),
        total: format_bytes(drive.capacity.total_bytes()).into(),
        used: format_bytes(drive.capacity.used_bytes()).into(),
        free: format_bytes(drive.capacity.free_bytes()).into(),
        usage: format!("{:.1}% used", usage * 100.0).into(),
        used_ratio: usage as f32,
    }
}

fn format_total(totals: Option<Capacity>, bytes: fn(Capacity) -> u64) -> String {
    totals.map_or_else(|| "—".to_owned(), |capacity| format_bytes(bytes(capacity)))
}
