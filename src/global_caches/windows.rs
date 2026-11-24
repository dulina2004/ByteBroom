use std::{
    ffi::OsString,
    io,
    os::windows::ffi::OsStringExt,
    path::{Component, Path, PathBuf, Prefix},
    ptr::null_mut,
};

use windows_sys::{
    Win32::{
        Storage::FileSystem::{GetLogicalDrives, QueryDosDeviceW},
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_ProgramData, FOLDERID_RoamingAppData,
            KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath,
        },
    },
    core::GUID,
};

use super::{
    model::Identity,
    providers::{Environment, VARIABLES},
};
use crate::scanner::{Cancellation, windows::Directory};

pub(super) fn environment() -> Environment {
    Environment {
        home: known_folder(&FOLDERID_Profile, "USERPROFILE"),
        local: known_folder(&FOLDERID_LocalAppData, "LOCALAPPDATA"),
        roaming: known_folder(&FOLDERID_RoamingAppData, "APPDATA"),
        program_data: known_folder(&FOLDERID_ProgramData, "PROGRAMDATA"),
        temp: std::env::var_os("TEMP").map(PathBuf::from),
        drives: local_drives(),
        variables: VARIABLES
            .iter()
            .filter_map(|&name| std::env::var_os(name).map(|value| (name, value)))
            .collect(),
    }
}

fn known_folder(id: &GUID, fallback: &str) -> Option<PathBuf> {
    let mut path = null_mut();
    // SAFETY: id is a known-folder GUID. Windows allocates the output, released below.
    let status =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY as u32, null_mut(), &mut path) };
    let known = if status >= 0 && !path.is_null() {
        // SAFETY: SHGetKnownFolderPath returns a terminated UTF-16 string.
        let mut length = 0;
        while unsafe { *path.add(length) } != 0 {
            length += 1;
        }
        Some(PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(path, length)
        })))
    } else {
        None
    };
    // SAFETY: the API documents that its output uses the COM task allocator.
    unsafe { CoTaskMemFree(path.cast()) };
    known.or_else(|| std::env::var_os(fallback).map(PathBuf::from))
}

fn local_drives() -> Vec<char> {
    // SAFETY: GetLogicalDrives has no arguments.
    let mask = unsafe { GetLogicalDrives() };
    (b'A'..=b'Z')
        .filter(|&letter| mask & (1 << (letter - b'A')) != 0 && native_volume(letter))
        .map(char::from)
        .collect()
}

fn native_volume(letter: u8) -> bool {
    let input = [u16::from(letter.to_ascii_uppercase()), u16::from(b':'), 0];
    let mut output = [0_u16; 1024];
    // SAFETY: the drive name is terminated and the output buffer has the supplied length.
    // This queries the local DOS device table before any path on that drive is opened.
    let length =
        unsafe { QueryDosDeviceW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    if length == 0 {
        return false;
    }
    let name = output.split(|&value| value == 0).next().unwrap_or_default();
    // Admit ordinary native volume mappings. Redirectors and SUBST mappings can hide
    // network shares or reparse points before the scanner has validated any path.
    let native = |prefix: &str| {
        name.iter()
            .copied()
            .zip(prefix.encode_utf16())
            .all(|(left, right)| left == right)
            && name.len() > prefix.len()
            && name[prefix.len()..]
                .iter()
                .all(|&unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit))
    };
    native("\\Device\\HarddiskVolume") || native("\\Device\\Ramdisk")
}

pub(crate) fn drive(path: &Path) -> Option<char> {
    if !path.is_absolute() {
        return None;
    }
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return None,
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let mut has_child = false;
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        let bytes = name.as_encoded_bytes();
        if bytes.iter().any(|&byte| {
            matches!(
                byte,
                0 | b':' | b'/' | b'\\' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
            )
        }) || bytes.last().is_some_and(|byte| matches!(byte, b'.' | b' '))
            || !has_child
                && [
                    b"Windows".as_slice(),
                    b"Program Files",
                    b"Program Files (x86)",
                ]
                .into_iter()
                .any(|system| bytes.eq_ignore_ascii_case(system))
            || [
                b".ssh".as_slice(),
                b".gnupg",
                b".aws",
                b"System Volume Information",
                b"$Recycle.Bin",
            ]
            .into_iter()
            .any(|sensitive| bytes.eq_ignore_ascii_case(sensitive))
        {
            return None;
        }
        has_child = true;
    }
    has_child.then(|| char::from(letter.to_ascii_uppercase()))
}

pub(super) fn open(
    path: &Path,
    cancellation: &Cancellation,
) -> io::Result<Option<(Directory, Identity)>> {
    let local = drive(path).is_some_and(|letter| native_volume(letter as u8));
    if !local {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only direct local-volume directories can be measured.",
        ));
    }
    let Some(directory) = Directory::open_root(path, cancellation)? else {
        return Ok(None);
    };
    let (volume, file, canonical) = directory.identity()?;
    Ok(Some((
        directory,
        Identity {
            volume,
            file,
            canonical,
        },
    )))
}
