use std::{
    ffi::{OsStr, OsString},
    io,
    mem::{offset_of, size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, Prefix},
    ptr::{null, null_mut},
};

use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_FULL_DIR_INFORMATION, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            FileFullDirectoryInformation, NtOpenFile, NtQueryDirectoryFile,
        },
    },
    Win32::{
        Foundation::{
            INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError,
            STATUS_NO_MORE_FILES, STATUS_NO_SUCH_FILE, STATUS_REPARSE_POINT_ENCOUNTERED,
            UNICODE_STRING,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileAttributeTagInfo, FileIdInfo,
            GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING, SYNCHRONIZE,
            VOLUME_NAME_GUID,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use super::Cancellation;
use crate::detectors::Facts;

const DIRECTORY_BUFFER_WORDS: usize = 1024;
const SHARE_MODE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
const NAME_OFFSET: usize = offset_of!(FILE_FULL_DIR_INFORMATION, FileName);

pub(crate) enum Entry {
    File(u64, Facts, bool),
    Directory(Vec<u16>),
    Link,
    Invalid,
}

pub(crate) struct Directory {
    handle: OwnedHandle,
    buffer: Option<Box<[u64; DIRECTORY_BUFFER_WORDS]>>,
    offset: usize,
    end: usize,
}

impl Directory {
    pub(crate) fn open_root(path: &Path, cancellation: &Cancellation) -> io::Result<Option<Self>> {
        let mut components = path.components();
        let Some(Component::Prefix(prefix)) = components.next() else {
            return Err(invalid_root());
        };
        if components.next() != Some(Component::RootDir) {
            return Err(invalid_root());
        }
        let mut anchor = match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)
                if crate::windows_fs::native_volume(letter) =>
            {
                OsString::from(format!("\\\\?\\{}:\\", char::from(letter)))
            }
            _ => return Err(invalid_root()),
        };
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let mut components = components.peekable();
        anchor.push(OsStr::new("\0"));
        let anchor = anchor.encode_wide().collect::<Vec<_>>();
        // SAFETY: anchor is a terminated local drive root. OPEN_EXISTING and access rights
        // permit only traversal, metadata, and optionally enumeration of an existing directory.
        let handle = unsafe {
            CreateFileW(
                anchor.as_ptr(),
                access(components.peek().is_none()),
                SHARE_MODE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW returned a valid handle that we own.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut directory = Self::validated(handle)?.ok_or_else(link_root)?;
        while let Some(component) = components.next() {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            let Component::Normal(name) = component else {
                return Err(invalid_root());
            };
            directory = directory
                .open_child(&wide_component(name)?, components.peek().is_none())?
                .ok_or_else(link_root)?;
        }
        Ok(Some(directory))
    }

    pub(super) fn open_child(&self, name: &[u16], enumerate: bool) -> io::Result<Option<Self>> {
        // A single literal component cannot escape through separators, a stream or '..'.
        if !valid_component(name) {
            return Err(invalid_root());
        }
        let length = u16::try_from(size_of_val(name)).map_err(|_| invalid_root())?;
        let mut name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: name.as_ptr().cast_mut(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.handle.as_raw_handle(),
            ObjectName: &mut name,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: null_mut(),
            SecurityQualityOfService: null_mut(),
        };
        let mut handle = null_mut();
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the directory handle and name buffer live through this synchronous open.
        // FILE_OPEN_REPARSE_POINT opens the reparse point itself if it reaches the filesystem.
        // We verify the returned handle before using it, including after an entry is replaced.
        let code = unsafe {
            NtOpenFile(
                &mut handle,
                access(enumerate),
                &attributes,
                &mut status,
                SHARE_MODE,
                FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )
        };
        if code == STATUS_REPARSE_POINT_ENCOUNTERED {
            return Ok(None);
        }
        if code < 0 {
            return Err(nt_error(code));
        }
        // SAFETY: successful NtOpenFile transferred ownership of the new handle.
        Self::validated(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn validated(handle: OwnedHandle) -> io::Result<Option<Self>> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: the owned handle is valid and info is writable for the specified length.
        let success = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileAttributeTagInfo,
                (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Ok(None);
        }
        if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Expected a directory.",
            ));
        }
        Ok(Some(Self {
            handle,
            buffer: None,
            offset: 0,
            end: 0,
        }))
    }

    pub(crate) fn next(
        &mut self,
        collect_evidence: bool,
        recognize_disk: bool,
    ) -> io::Result<Option<Entry>> {
        loop {
            if self.offset == self.end && !self.refill()? {
                return Ok(None);
            }
            let buffer = self.buffer.as_ref().expect("refill installed the buffer");
            // SAFETY: all bytes are initialized, the allocation is live, and refill limits end
            // to the number of bytes written within this allocation.
            let data =
                unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), self.end) };
            let (advance, entry) =
                parse_record(&data[self.offset..], collect_evidence, recognize_disk)?;
            self.offset = if advance == 0 {
                self.end
            } else {
                self.offset + advance
            };
            if entry.is_some() {
                return Ok(entry);
            }
        }
    }

    pub(crate) fn identity(&self) -> io::Result<(u64, [u8; 16], std::path::PathBuf)> {
        use std::os::windows::ffi::OsStringExt;

        let mut file = FILE_ID_INFO::default();
        // SAFETY: the directory handle is live and file is a writable fixed-size output.
        let success = unsafe {
            GetFileInformationByHandleEx(
                self.handle.as_raw_handle(),
                FileIdInfo,
                (&mut file as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut name = vec![0_u16; 512];
        loop {
            // SAFETY: the writable UTF-16 buffer has the length passed to Windows.
            let length = unsafe {
                GetFinalPathNameByHandleW(
                    self.handle.as_raw_handle(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    VOLUME_NAME_GUID,
                )
            } as usize;
            if length == 0 {
                return Err(io::Error::last_os_error());
            }
            if length < name.len() {
                return Ok((
                    file.VolumeSerialNumber,
                    file.FileId.Identifier,
                    OsString::from_wide(&name[..length]).into(),
                ));
            }
            if length > 32768 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The volume path is too long to verify.",
                ));
            }
            name.resize(length + 1, 0);
        }
    }

    fn refill(&mut self) -> io::Result<bool> {
        let first = self.buffer.is_none();
        let buffer = self
            .buffer
            .get_or_insert_with(|| Box::new([0; DIRECTORY_BUFFER_WORDS]));
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the owned directory handle is synchronous. The aligned, writable buffer and
        // status block remain live through the query; no APC or completion event is requested.
        let code = unsafe {
            NtQueryDirectoryFile(
                self.handle.as_raw_handle(),
                null_mut(),
                None,
                null(),
                &mut status,
                buffer.as_mut_ptr().cast(),
                size_of_val(buffer.as_ref()) as u32,
                FileFullDirectoryInformation,
                false,
                null(),
                first,
            )
        };
        if code == STATUS_NO_MORE_FILES || (first && code == STATUS_NO_SUCH_FILE) {
            return Ok(false);
        }
        if code < 0 {
            return Err(nt_error(code));
        }
        if status.Information == 0 || status.Information > size_of_val(buffer.as_ref()) {
            return Err(invalid_record());
        }
        self.offset = 0;
        self.end = status.Information;
        Ok(true)
    }
}

fn parse_record(
    data: &[u8],
    collect_evidence: bool,
    recognize_disk: bool,
) -> io::Result<(usize, Option<Entry>)> {
    let header = data.get(..NAME_OFFSET).ok_or_else(invalid_record)?;
    // The fixed-size fields below all precede FileName in FILE_FULL_DIR_INFORMATION.
    let read = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
    let next = read(offset_of!(FILE_FULL_DIR_INFORMATION, NextEntryOffset)) as usize;
    let length = read(offset_of!(FILE_FULL_DIR_INFORMATION, FileNameLength)) as usize;
    let record_end = NAME_OFFSET.checked_add(length).ok_or_else(invalid_record)?;
    if length == 0
        || !length.is_multiple_of(2)
        || (next != 0 && (next < record_end || next >= data.len() || !next.is_multiple_of(8)))
    {
        return Err(invalid_record());
    }
    let name = data
        .get(NAME_OFFSET..record_end)
        .ok_or_else(invalid_record)?;
    if name == [b'.', 0] || name == [b'.', 0, b'.', 0] {
        return Ok((next, None));
    }
    let flags = read(offset_of!(FILE_FULL_DIR_INFORMATION, FileAttributes));
    let entry = if flags & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Entry::Link
    } else if flags & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Entry::Directory(
            name.as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect(),
        )
    } else {
        let offset = offset_of!(FILE_FULL_DIR_INFORMATION, EndOfFile);
        let length = i64::from_le_bytes(header[offset..offset + 8].try_into().unwrap());
        u64::try_from(length).map_or(Entry::Invalid, |bytes| {
            let facts = if collect_evidence {
                Facts::from_windows_file(name)
            } else {
                Facts::default()
            };
            Entry::File(bytes, facts, recognize_disk && disk_image(name))
        })
    };
    Ok((next, Some(entry)))
}

fn disk_image(name: &[u8]) -> bool {
    let wide = name.as_chunks::<2>().0;
    [".img", ".qcow2", ".vhd", ".vhdx"]
        .into_iter()
        .any(|suffix| {
            wide.len() > suffix.len()
                && wide[wide.len() - suffix.len()..]
                    .iter()
                    .zip(suffix.bytes())
                    .all(|(unit, byte)| unit[1] == 0 && unit[0].eq_ignore_ascii_case(&byte))
        })
}

fn access(enumerate: bool) -> u32 {
    FILE_READ_ATTRIBUTES
        | FILE_TRAVERSE
        | SYNCHRONIZE
        | if enumerate { FILE_LIST_DIRECTORY } else { 0 }
}

fn wide_component(name: &OsStr) -> io::Result<Vec<u16>> {
    let name = name.encode_wide().collect::<Vec<_>>();
    if valid_component(&name) {
        Ok(name)
    } else {
        Err(invalid_root())
    }
}

fn valid_component(name: &[u16]) -> bool {
    !name.is_empty()
        && name != [46]
        && name != [46, 46]
        && !name.iter().any(|&unit| matches!(unit, 0 | 47 | 58 | 92))
}

fn invalid_root() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Choose an absolute folder path on a direct local Windows drive.",
    )
}

fn link_root() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "The selected path contains a link, junction, or other reparse point.",
    )
}

fn invalid_record() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows returned invalid directory metadata.",
    )
}

fn nt_error(status: i32) -> io::Error {
    // SAFETY: this converts an NTSTATUS value to a Win32 error code.
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}
