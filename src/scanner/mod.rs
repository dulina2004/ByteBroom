use crate::detectors::Results;
#[cfg(windows)]
use crate::detectors::{Collector, Context, Measurement};
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
    pub(crate) projects: usize,
    pub(crate) artifact_candidates: usize,
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
    pub(crate) developer: Results,
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
        #[cfg(windows)]
        developer: Collector::default(),
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
    let partial = !matches!(outcome, ScanOutcome::Completed) || scan.progress.inaccessible != 0;
    #[cfg(windows)]
    let developer = scan.developer.finish(partial);
    #[cfg(not(windows))]
    let developer = Results {
        partial,
        ..Results::default()
    };
    scan.progress.projects = developer.projects.len();
    scan.progress.artifact_candidates = developer.artifacts.len();
    scan.progress.elapsed = scan.started.elapsed();
    report(scan.progress);
    ScanResult {
        root: request.root,
        progress: scan.progress,
        top_directories: scan.top,
        outcome,
        developer,
    }
}

struct Traversal {
    progress: ScanProgress,
    top: Vec<DirectorySize>,
    #[cfg(windows)]
    developer: Collector,
    started: Instant,
    last_report: Duration,
    interval: Duration,
}

impl Traversal {
    #[cfg(windows)]
    fn rank_directory(&mut self, child: DirectorySize) {
        let index = self.top.partition_point(|previous| {
            previous.logical_bytes > child.logical_bytes
                || (previous.logical_bytes == child.logical_bytes && previous.name <= child.name)
        });
        if index < TOP_DIRECTORIES {
            if self.top.len() == TOP_DIRECTORIES {
                self.top.pop();
            }
            self.top.insert(index, child);
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
    let mut path = request.root.clone();
    let mut stack = vec![Frame::new(root, Context::root(&path), false)];
    scan.progress.directories = 1;
    let mut root_error = None;

    while !stack.is_empty() {
        if cancellation.is_cancelled() {
            break;
        }
        let is_root = stack.len() == 1;
        let directory = stack.last_mut().expect("directory stack is not empty");
        let entry = directory
            .directory
            .next(directory.context.wants_file_evidence());
        if cancellation.is_cancelled() {
            break;
        }
        match entry {
            Ok(Some(Entry::File(bytes, evidence))) => {
                scan.progress.files += 1;
                scan.progress.logical_bytes = scan.progress.logical_bytes.saturating_add(bytes);
                directory.measurement.files += 1;
                directory.measurement.logical_bytes =
                    directory.measurement.logical_bytes.saturating_add(bytes);
                directory.context.file(evidence);
            }
            Ok(Some(Entry::Link)) => scan.progress.skipped_links += 1,
            Ok(Some(Entry::Invalid)) => {
                scan.progress.inaccessible += 1;
                directory.measurement.inaccessible += 1;
            }
            Ok(Some(Entry::Directory(name))) => {
                let child = directory.directory.open_child(&name, true);
                if cancellation.is_cancelled() {
                    break;
                }
                match child {
                    Ok(Some(child)) => {
                        let examine = directory.context.examines_children();
                        let component = examine.then(|| OsString::from_wide(&name));
                        let context = directory.context.child(
                            component.as_deref(),
                            path.file_name(),
                            scan.progress.directories,
                        );
                        let tracked = examine || is_root;
                        if tracked {
                            path.push(component.unwrap_or_else(|| OsString::from_wide(&name)));
                        }
                        scan.progress.directories += 1;
                        stack.push(Frame::new(child, context, tracked));
                    }
                    Ok(None) => scan.progress.skipped_links += 1,
                    Err(_) => {
                        scan.progress.inaccessible += 1;
                        directory.measurement.inaccessible += 1;
                    }
                }
            }
            result => {
                if let Err(error) = result {
                    if is_root {
                        root_error = Some(error);
                        break;
                    }
                    scan.progress.inaccessible += 1;
                    directory.measurement.inaccessible += 1;
                }
                finish_frame(&mut stack, &mut path, scan, true);
            }
        }
        scan.report_if_due(report);
    }
    while !stack.is_empty() {
        finish_frame(&mut stack, &mut path, scan, false);
    }
    root_error.map_or(Ok(()), Err)
}

#[cfg(windows)]
struct Frame {
    directory: windows::Directory,
    context: Context,
    measurement: Measurement,
    tracked: bool,
}

#[cfg(windows)]
impl Frame {
    fn new(directory: windows::Directory, context: Context, tracked: bool) -> Self {
        Self {
            directory,
            context,
            measurement: Measurement {
                directories: 1,
                ..Measurement::default()
            },
            tracked,
        }
    }
}

#[cfg(windows)]
fn finish_frame(stack: &mut Vec<Frame>, path: &mut PathBuf, scan: &mut Traversal, finished: bool) {
    let frame = stack.pop().expect("directory stack is not empty");
    let measured = frame.measurement;
    let findings = scan.developer.observe(
        frame.context,
        path,
        scan.progress.directories - 1,
        measured,
        finished,
    );
    scan.progress.projects += usize::from(findings.project);
    scan.progress.artifact_candidates += usize::from(findings.candidate);
    if stack.len() == 1
        && let Some(name) = path.file_name()
    {
        scan.rank_directory(DirectorySize {
            name: name.to_owned(),
            logical_bytes: measured.logical_bytes,
        });
    }
    if frame.tracked {
        path.pop();
    }
    if let Some(parent) = stack.last_mut() {
        parent.measurement.logical_bytes = parent
            .measurement
            .logical_bytes
            .saturating_add(measured.logical_bytes);
        parent.measurement.files += measured.files;
        parent.measurement.directories += measured.directories;
        parent.measurement.inaccessible += measured.inaccessible;
    }
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
