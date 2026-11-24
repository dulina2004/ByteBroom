use std::{io, marker::PhantomData, ptr::null_mut, rc::Rc};

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
