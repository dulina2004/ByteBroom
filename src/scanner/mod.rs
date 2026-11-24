use std::{
    ffi::OsString,
    io,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(all(test, windows))]
mod tests;
#[cfg(windows)]
mod windows;

pub(crate) const TOP_DIRECTORIES: usize = 12;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(125);

pub(crate) struct ScanRequest {
    pub(crate) root: PathBuf,
}

#[derive(Default)]
pub(crate) struct Cancellation(AtomicBool);

impl Cancellation {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScanProgress {
    /// Sum of logical file lengths. Sparse or compressed files may occupy less physical space.
    pub(crate) logical_bytes: u64,
    pub(crate) files: u64,
    /// Includes the root and each child directory opened for enumeration.
    pub(crate) directories: u64,
    pub(crate) inaccessible: u64,
    pub(crate) skipped_links: u64,
    pub(crate) elapsed: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DirectorySize {
    pub(crate) name: OsString,
    pub(crate) logical_bytes: u64,
}

#[derive(Debug)]
pub(crate) enum ScanOutcome {
    Completed,
    Cancelled,
    Failed(io::Error),
}

#[derive(Debug)]
pub(crate) struct ScanResult {
    pub(crate) root: PathBuf,
    pub(crate) progress: ScanProgress,
    pub(crate) top_directories: Vec<DirectorySize>,
    pub(crate) outcome: ScanOutcome,
}

pub(crate) fn scan(
    request: ScanRequest,
    cancellation: &Cancellation,
    report: impl FnMut(ScanProgress),
) -> ScanResult {
    scan_with_interval(request, cancellation, report, PROGRESS_INTERVAL)
}

fn scan_with_interval(
    request: ScanRequest,
    cancellation: &Cancellation,
    mut report: impl FnMut(ScanProgress),
    interval: Duration,
) -> ScanResult {
    let mut scan = Traversal {
        progress: ScanProgress::default(),
        top: Vec::with_capacity(TOP_DIRECTORIES),
        current_child: None,
        started: Instant::now(),
        last_report: Duration::ZERO,
        interval,
    };
    report(scan.progress);
    let mut outcome = ScanOutcome::Cancelled;
    if !cancellation.is_cancelled() {
        let result = traverse(&request, cancellation, &mut scan, &mut report);
        if !cancellation.is_cancelled() {
            outcome = result.map_or_else(ScanOutcome::Failed, |()| ScanOutcome::Completed);
        }
    }
    scan.finish_child();
    scan.progress.elapsed = scan.started.elapsed();
    report(scan.progress);
    ScanResult {
        root: request.root,
        progress: scan.progress,
        top_directories: scan.top,
        outcome,
    }
}

struct Traversal {
    progress: ScanProgress,
    top: Vec<DirectorySize>,
    current_child: Option<DirectorySize>,
    started: Instant,
    last_report: Duration,
    interval: Duration,
}

impl Traversal {
    fn finish_child(&mut self) {
        if let Some(child) = self.current_child.take() {
            let index = self.top.partition_point(|previous| {
                previous.logical_bytes > child.logical_bytes
                    || (previous.logical_bytes == child.logical_bytes
                        && previous.name <= child.name)
            });
            if index < TOP_DIRECTORIES {
                if self.top.len() == TOP_DIRECTORIES {
                    self.top.pop();
                }
                self.top.insert(index, child);
            }
        }
    }

    #[cfg(windows)]
    fn report_if_due(&mut self, report: &mut impl FnMut(ScanProgress)) {
        let elapsed = self.started.elapsed();
        if elapsed - self.last_report >= self.interval {
            self.progress.elapsed = elapsed;
            self.last_report = elapsed;
            report(self.progress);
        }
    }
}

#[cfg(windows)]
fn traverse(
    request: &ScanRequest,
    cancellation: &Cancellation,
    scan: &mut Traversal,
    report: &mut impl FnMut(ScanProgress),
) -> io::Result<()> {
    use std::os::windows::ffi::OsStringExt;
    use windows::{Directory, Entry};

    let _error_mode = crate::windows_fs::ErrorMode::suppress_dialogs()?;
    let Some(root) = Directory::open_root(&request.root, cancellation)? else {
        return Ok(());
    };
    let mut stack = vec![root];
    scan.progress.directories = 1;

    while let Some(directory) = stack.last_mut() {
        if cancellation.is_cancelled() {
            break;
        }
        let entry = directory.next();
        if cancellation.is_cancelled() {
            break;
        }
        match entry {
            Ok(Some(Entry::File(bytes))) => {
                scan.progress.files += 1;
                scan.progress.logical_bytes = scan.progress.logical_bytes.saturating_add(bytes);
                if let Some(child) = &mut scan.current_child {
                    child.logical_bytes = child.logical_bytes.saturating_add(bytes);
                }
            }
            Ok(Some(Entry::Link)) => scan.progress.skipped_links += 1,
            Ok(Some(Entry::Invalid)) => scan.progress.inaccessible += 1,
            Ok(Some(Entry::Directory(name))) => {
                let child = directory.open_child(&name, true);
                if cancellation.is_cancelled() {
                    break;
                }
                match child {
                    Ok(Some(child)) => {
                        if stack.len() == 1 {
                            scan.current_child = Some(DirectorySize {
                                name: OsString::from_wide(&name),
                                logical_bytes: 0,
                            });
                        }
                        scan.progress.directories += 1;
                        stack.push(child);
                    }
                    Ok(None) => scan.progress.skipped_links += 1,
                    Err(_) => scan.progress.inaccessible += 1,
                }
            }
            result => {
                if let Err(error) = result {
                    if stack.len() == 1 {
                        return Err(error);
                    }
                    scan.progress.inaccessible += 1;
                }
                stack.pop();
                if stack.len() == 1 {
                    scan.finish_child();
                }
            }
        }
        scan.report_if_due(report);
    }
    Ok(())
}

#[cfg(not(windows))]
fn traverse(
    _: &ScanRequest,
    _: &Cancellation,
    scan: &mut Traversal,
    _: &mut impl FnMut(ScanProgress),
) -> io::Result<()> {
    let _ = (scan.last_report, scan.interval);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Scanning is currently available on Windows only.",
    ))
}
