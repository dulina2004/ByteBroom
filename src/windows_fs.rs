use std::{io, marker::PhantomData, ptr::null_mut, rc::Rc};
use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;

pub(crate) fn native_volume(letter: u8) -> bool {
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
    native("\\Device\\HarddiskVolume") || native("\\Device\\Ramdisk") || native("\\Device\\CdRom")
}

use windows_sys::Win32::System::Diagnostics::Debug::{
    GetThreadErrorMode, SEM_FAILCRITICALERRORS, SetThreadErrorMode,
};

pub(crate) struct ErrorMode {
    previous: u32,
    // The mode must be restored on the same thread where it was set.
    thread_local: PhantomData<Rc<()>>,
}

impl ErrorMode {
    pub(crate) fn suppress_dialogs() -> io::Result<Self> {
        let mut previous = 0;
        // SAFETY: the mode applies only to this thread and previous is a writable output.
        let success = unsafe {
            SetThreadErrorMode(GetThreadErrorMode() | SEM_FAILCRITICALERRORS, &mut previous)
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            previous,
            thread_local: PhantomData,
        })
    }
}

impl Drop for ErrorMode {
    fn drop(&mut self) {
        // SAFETY: this guard cannot change threads, and the output is optional.
        unsafe { SetThreadErrorMode(self.previous, null_mut()) };
    }
}
