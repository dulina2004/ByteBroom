use std::rc::Rc;

use slint::{ComponentHandle, PlatformError, VecModel};

use crate::models::{Drive, MOCK_DRIVES};

slint::include_modules!();

pub(crate) fn run() -> Result<(), PlatformError> {
    let window = AppWindow::new()?;
    let drives = MOCK_DRIVES.iter().map(drive_card).collect::<Vec<_>>();
    let total_gb: u32 = MOCK_DRIVES.iter().map(|drive| drive.total_gb).sum();
    let used_gb: u32 = MOCK_DRIVES.iter().map(|drive| drive.used_gb).sum();
    let reclaimable_gb: u32 = MOCK_DRIVES.iter().map(|drive| drive.reclaimable_gb).sum();

    window.set_drives(Rc::new(VecModel::from(drives)).into());
    window.set_total_space(format_gb(total_gb).into());
    window.set_used_space(format_gb(used_gb).into());
    window.set_free_space(format_gb(total_gb - used_gb).into());
    window.set_reclaimable_space(format_gb(reclaimable_gb).into());
    window.set_app_version(env!("CARGO_PKG_VERSION").into());
    window.run()
}

fn drive_card(drive: &Drive) -> DriveCardData {
    DriveCardData {
        letter: drive.letter.into(),
        name: drive.name.into(),
        total: format_gb(drive.total_gb).into(),
        used: format_gb(drive.used_gb).into(),
        free: format_gb(drive.free_gb()).into(),
        reclaimable: format_gb(drive.reclaimable_gb).into(),
        used_ratio: drive.used_gb as f32 / drive.total_gb as f32,
        reclaimable_ratio: drive.reclaimable_gb as f32 / drive.total_gb as f32,
    }
}

fn format_gb(gb: u32) -> String {
    if gb >= 1000 {
        format!("{:.2} TB", f64::from(gb) / 1000.0)
    } else {
        format!("{gb} GB")
    }
}
