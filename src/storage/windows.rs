use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Storage::FileSystem::{
        DISK_SPACE_INFORMATION, GetDiskFreeSpaceExW, GetDiskSpaceInformationW, GetDriveTypeW,
        GetLogicalDrives, GetVolumeInformationW,
    },
    System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    },
};

use super::{Capacity, Discovery, Drive};
use crate::windows_fs::ErrorMode;

pub(super) fn discover() -> io::Result<Discovery> {
    let _error_mode = ErrorMode::suppress_dialogs()?;
    // SAFETY: GetLogicalDrives has no arguments and returns the current letter bitmask.
    let letters = unsafe { GetLogicalDrives() };
    if letters == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut result = Discovery::default();
    for (index, ascii_letter) in (b'A'..=b'Z').enumerate() {
        if letters & (1 << index) == 0 {
            continue;
        }

        let letter = char::from(ascii_letter);
        let root = [
            u16::from(ascii_letter),
            u16::from(b':'),
            u16::from(b'\\'),
            0,
        ];
        if let Some(capacity) = read_capacity(&root) {
            result.drives.push(Drive {
                letter,
                label: volume_label(&root),
                kind: drive_kind(&root),
                capacity,
            });
        } else {
            result.unavailable.push(letter);
        }
    }
    Ok(result)
}

fn drive_kind(root: &[u16; 4]) -> &'static str {
    // SAFETY: root is a null-terminated Windows drive root.
    match unsafe { GetDriveTypeW(root.as_ptr()) } {
        DRIVE_FIXED => "Fixed drive",
        DRIVE_REMOVABLE => "Removable drive",
        DRIVE_REMOTE => "Network drive",
        DRIVE_CDROM => "Optical drive",
        DRIVE_RAMDISK => "RAM disk",
        _ => "Drive",
    }
}

fn volume_label(root: &[u16; 4]) -> String {
    let mut label = [0_u16; 261];
    // SAFETY: root is null-terminated, label has the supplied size, and the omitted outputs
    // are optional.
    let success = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            0,
        )
    };
    if success == 0 {
        return String::new();
    }
    let end = label
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(label.len());
    String::from_utf16_lossy(&label[..end])
}

fn read_capacity(root: &[u16; 4]) -> Option<Capacity> {
    let mut space = DISK_SPACE_INFORMATION::default();
    // SAFETY: root is null-terminated and space is a writable output of the required type.
    if unsafe { GetDiskSpaceInformationW(root.as_ptr(), &mut space) } == 0 {
        let allocation_bytes =
            u64::from(space.SectorsPerAllocationUnit) * u64::from(space.BytesPerSector);
        let actual = space
            .ActualTotalAllocationUnits
            .checked_mul(allocation_bytes)
            .zip(
                space
                    .ActualAvailableAllocationUnits
                    .checked_mul(allocation_bytes),
            )
            .and_then(|(total, free)| Capacity::new(total, free));
        if actual.is_some() {
            return actual;
        }
    }

    // Some providers, such as network shares, only support the older query. Pair the
    // caller's total and free bytes so quotas affect both figures consistently.
    let (mut free, mut total) = (0, 0);
    // SAFETY: root is null-terminated, both outputs are writable, and the last is optional.
    let success = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, null_mut()) };
    if success == 0 {
        return None;
    }
    Capacity::new(total, free)
}
