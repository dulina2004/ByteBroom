use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, ptr::null};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows_sys::Win32::{
    System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    },
    UI::Shell::{
        BIF_EDITBOX, BIF_NONEWFOLDERBUTTON, BIF_NOTRANSLATETARGETS, BIF_RETURNONLYFSDIRS,
        BROWSEINFOW, GPFIDL_DEFAULT, SHBrowseForFolderW, SHGetPathFromIDListEx,
    },
};

use super::AppWindow;

pub(super) fn owner(window: &AppWindow) -> Result<isize, String> {
    let handle = window.window().window_handle();
    match handle.window_handle().ok().map(|handle| handle.as_raw()) {
        Some(RawWindowHandle::Win32(handle)) => Ok(handle.hwnd.get()),
        _ => Err("The Windows folder picker needs an active ByteBroom window.".into()),
    }
}

pub(super) fn choose(owner: isize) -> Result<Option<PathBuf>, String> {
    // SAFETY: this runs on a dedicated STA thread. The guard balances COM initialization.
    let status = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if status < 0 {
        return Err(format!(
            "Windows could not initialize the folder picker (0x{status:08X})."
        ));
    }
    let _apartment = Apartment;

    let title = "Choose a folder for a read-only scan.\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut name = [0_u16; 260];
    let browse = BROWSEINFOW {
        hwndOwner: owner as *mut _,
        pszDisplayName: name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        // The classic tree picker omits the shell's context menus and drag/drop. Hide its
        // creation button explicitly and preserve shortcut paths for scanner validation.
        ulFlags: BIF_EDITBOX
            | BIF_NONEWFOLDERBUTTON
            | BIF_NOTRANSLATETARGETS
            | BIF_RETURNONLYFSDIRS,
        ..Default::default()
    };
    // SAFETY: the owner stays alive while the worker runs and the buffers outlive the dialog.
    let item = unsafe { SHBrowseForFolderW(&browse) };
    if item.is_null() {
        return Ok(None);
    }
    let mut path = vec![0_u16; 32768];
    // SAFETY: the dialog returned an owned PIDL; the supplied path buffer is writable.
    let success = unsafe {
        let result =
            SHGetPathFromIDListEx(item, path.as_mut_ptr(), path.len() as u32, GPFIDL_DEFAULT);
        CoTaskMemFree(item.cast());
        result
    };
    let end = path
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(path.len());
    if success == 0 || end == 0 || end == path.len() {
        return Err("The selected item does not have a usable filesystem path.".into());
    }
    Ok(Some(PathBuf::from(OsString::from_wide(&path[..end]))))
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: the guard was created after COM initialized on this same thread.
        unsafe { CoUninitialize() };
    }
}
