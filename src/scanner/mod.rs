#[cfg(windows)]
use crate::detectors::{Collector, Context, Measurement};
use crate::{analysis::Folder, detectors::Results};
use std::{
    ffi::OsString,
    io,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(all(test, windows))]
pub(crate) mod tests;
#[cfg(windows)]
pub(crate) mod windows;

pub(crate) const TOP_DIRECTORIES: usize = 12;
#[cfg(windows)]
pub(crate) const SUMMARY_DEPTH: usize = 3;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopeIdentity(pub(crate) u64, pub(crate) [u8; 16], pub(crate) PathBuf);

pub(crate) type RecordedIdentities = std::collections::HashMap<PathBuf, ScopeIdentity>;

#[derive(Debug)]
pub(crate) struct ScanResult {
    pub(crate) root: PathBuf,
    pub(crate) progress: ScanProgress,
    pub(crate) top_directories: Vec<DirectorySize>,
    pub(crate) hierarchy: Option<Folder>,
    pub(crate) identity: Option<ScopeIdentity>,
    pub(crate) cleanup_identities: RecordedIdentities,
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
        hierarchy: None,
        identity: None,
        cleanup_identities: RecordedIdentities::new(),
        #[cfg(windows)]
        developer: Some(Collector::default()),
        #[cfg(windows)]
        avd: None,
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
    let developer = scan
        .developer
        .expect("project scans collect evidence")
        .finish(partial);
    #[cfg(not(windows))]
    let developer = Results {
        partial,
        ..Results::default()
    };
    scan.progress.projects = developer.projects.len();
    scan.progress.artifact_candidates = developer.artifacts.len();
    scan.progress.elapsed = scan.started.elapsed();
    report(scan.progress);
    let top_directories = scan
        .hierarchy
        .iter()
        .flat_map(|root| &root.children)
        .map(|child| DirectorySize {
            name: child.name.clone(),
            logical_bytes: child.measurement.logical_bytes,
        })
        .collect();
    ScanResult {
        root: request.root,
        progress: scan.progress,
        top_directories,
        hierarchy: scan.hierarchy,
        identity: scan.identity,
        cleanup_identities: scan.cleanup_identities,
        outcome,
        developer,
    }
}

struct Traversal {
    progress: ScanProgress,
    hierarchy: Option<Folder>,
    identity: Option<ScopeIdentity>,
    cleanup_identities: RecordedIdentities,
    #[cfg(windows)]
    developer: Option<Collector>,
    #[cfg(windows)]
    avd: Option<AvdSizes>,
    started: Instant,
    last_report: Duration,
    interval: Duration,
}

impl Traversal {
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
    let _error_mode = crate::windows_fs::ErrorMode::suppress_dialogs()?;
    let Some(root) = windows::Directory::open_root(&request.root, cancellation)? else {
        return Ok(());
    };
    scan.identity = root
        .identity()
        .ok()
        .map(|(volume, file, path)| ScopeIdentity(volume, file, path));
    walk(root, request.root.clone(), None, cancellation, scan, report)
}

#[cfg(windows)]
fn walk(
    root: windows::Directory,
    mut path: PathBuf,
    exclusions: Option<&Exclusions>,
    cancellation: &Cancellation,
    scan: &mut Traversal,
    report: &mut impl FnMut(ScanProgress),
) -> io::Result<()> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Entry;

    let mut stack = vec![Frame::new(root, Context::root(&path), false, exclusions)];
    scan.progress.directories = 1;
    let mut root_error = None;

    while !stack.is_empty() {
        if cancellation.is_cancelled() {
            break;
        }
        let depth = stack.len();
        let is_root = depth == 1;
        let directory = stack.last_mut().expect("directory stack is not empty");
        let entry = directory.directory.next(
            scan.developer.is_some() && directory.context.wants_file_evidence(),
            is_root && scan.avd.is_some(),
        );
        if cancellation.is_cancelled() {
            break;
        }
        match entry {
            Ok(Some(Entry::File(bytes, evidence, disk))) => {
                scan.progress.files += 1;
                scan.progress.logical_bytes = scan.progress.logical_bytes.saturating_add(bytes);
                directory.measurement.files += 1;
                directory.measurement.logical_bytes =
                    directory.measurement.logical_bytes.saturating_add(bytes);
                directory.direct_bytes = directory.direct_bytes.saturating_add(bytes);
                directory.context.file(evidence);
                if disk && let Some(avd) = &mut scan.avd {
                    avd.disk_bytes = avd.disk_bytes.saturating_add(bytes);
                    avd.disk_files += 1;
                }
            }
            Ok(Some(Entry::Link)) => scan.progress.skipped_links += 1,
            Ok(Some(Entry::NamedFile(..))) => unreachable!("scan enumeration omits file names"),
            Ok(Some(Entry::Invalid)) => {
                scan.progress.inaccessible += 1;
                directory.measurement.inaccessible += 1;
            }
            Ok(Some(Entry::Directory(name))) => {
                let exclusion = directory
                    .exclusions
                    .and_then(|branch| branch.children.get(&OsString::from_wide(&name)));
                if exclusion.is_some_and(|branch| branch.skip) {
                    scan.report_if_due(report);
                    continue;
                }
                let child = directory.directory.open_child(&name, true);
                if cancellation.is_cancelled() {
                    break;
                }
                match child {
                    Ok(Some(child)) => {
                        let examine =
                            scan.developer.is_some() && directory.context.examines_children();
                        let component = examine.then(|| OsString::from_wide(&name));
                        let context = directory.context.child(
                            component.as_deref(),
                            path.file_name(),
                            scan.progress.directories,
                        );
                        let tracked = examine
                            || is_root
                            || scan.developer.is_some() && depth <= SUMMARY_DEPTH;
                        if tracked {
                            path.push(component.unwrap_or_else(|| OsString::from_wide(&name)));
                        }
                        scan.progress.directories += 1;
                        stack.push(Frame::new(child, context, tracked, exclusion));
                    }
                    Ok(None) => scan.progress.skipped_links += 1,
                    Err(_) => {
                        scan.progress.inaccessible += 1;
                        directory.measurement.inaccessible += 1;
                    }
                }
            }
            result => {
                let finished = result.is_ok();
                if let Err(error) = result {
                    if is_root {
                        root_error = Some(error);
                        break;
                    }
                    scan.progress.inaccessible += 1;
                    directory.measurement.inaccessible += 1;
                }
                finish_frame(&mut stack, &mut path, scan, finished);
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
struct Frame<'a> {
    directory: windows::Directory,
    context: Context,
    measurement: Measurement,
    direct_bytes: u64,
    children: Vec<Folder>,
    omitted: u64,
    omitted_count: u64,
    tracked: bool,
    exclusions: Option<&'a Exclusions>,
}

#[cfg(windows)]
impl<'a> Frame<'a> {
    fn new(
        directory: windows::Directory,
        context: Context,
        tracked: bool,
        exclusions: Option<&'a Exclusions>,
    ) -> Self {
        Self {
            directory,
            context,
            measurement: Measurement {
                directories: 1,
                ..Measurement::default()
            },
            direct_bytes: 0,
            children: Vec::new(),
            omitted: 0,
            omitted_count: 0,
            tracked,
            exclusions,
        }
    }
}

#[cfg(windows)]
fn finish_frame(
    stack: &mut Vec<Frame<'_>>,
    path: &mut PathBuf,
    scan: &mut Traversal,
    finished: bool,
) {
    let mut frame = stack.pop().expect("directory stack is not empty");
    let measured = frame.measurement;
    if let Some(collector) = &mut scan.developer {
        let findings = collector.observe(
            frame.context,
            path,
            scan.progress.directories - 1,
            measured,
            finished,
        );
        if finished
            && (findings.project || findings.candidate && crate::cleanup::tracked(path))
            && scan.cleanup_identities.len() < crate::cleanup::IDENTITY_LIMIT
            && let Ok((volume, file, canonical)) = frame.directory.identity()
            && file != [0; 16]
        {
            scan.cleanup_identities
                .insert(path.to_owned(), ScopeIdentity(volume, file, canonical));
        }
        scan.progress.projects += usize::from(findings.project);
        scan.progress.artifact_candidates += usize::from(findings.candidate);
    }
    if stack.len() == 1
        && let Some(name) = path.file_name()
        && let Some(avd) = &mut scan.avd
        && name.as_encoded_bytes().eq_ignore_ascii_case(b"snapshots")
    {
        avd.snapshot_bytes = Some(measured.logical_bytes);
    }
    if scan.developer.is_some() && stack.len() <= SUMMARY_DEPTH {
        let folder = Folder {
            name: path.file_name().unwrap_or(path.as_os_str()).to_owned(),
            measurement: measured,
            direct_bytes: frame.direct_bytes,
            omitted_bytes: frame.omitted,
            omitted_count: frame.omitted_count,
            expanded: stack.len() < SUMMARY_DEPTH,
            complete: finished && measured.inaccessible == 0,
            children: std::mem::take(&mut frame.children),
        };
        let limit = if stack.len() == 1 { TOP_DIRECTORIES } else { 6 };
        if let Some(parent) = stack.last_mut() {
            parent.retain(folder, limit);
        } else {
            scan.hierarchy = Some(folder);
        }
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

#[cfg(windows)]
impl Frame<'_> {
    fn retain(&mut self, folder: Folder, limit: usize) {
        let index = self.children.partition_point(|item| {
            (
                std::cmp::Reverse(item.measurement.logical_bytes),
                &item.name,
            ) <= (
                std::cmp::Reverse(folder.measurement.logical_bytes),
                &folder.name,
            )
        });
        let omitted = if index < limit {
            let omitted = (self.children.len() == limit)
                .then(|| self.children.pop())
                .flatten();
            self.children.insert(index, folder);
            omitted
        } else {
            Some(folder)
        };
        if let Some(omitted) = omitted {
            self.omitted = self
                .omitted
                .saturating_add(omitted.measurement.logical_bytes);
            self.omitted_count += 1;
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AvdSizes {
    pub(crate) disk_files: u64,
    pub(crate) disk_bytes: u64,
    pub(crate) snapshot_bytes: Option<u64>,
}

/// Directory boundaries relative to an already validated measurement root.
#[cfg(windows)]
#[derive(Default)]
pub(crate) struct Exclusions {
    children: std::collections::HashMap<OsString, Self>,
    skip: bool,
}

#[cfg(windows)]
impl Exclusions {
    pub(crate) fn insert(&mut self, relative: &std::path::Path) {
        let mut branch = self;
        for name in relative {
            branch = branch.children.entry(name.to_owned()).or_default();
        }
        branch.skip = true;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

#[cfg(windows)]
pub(crate) fn measure(
    root: windows::Directory,
    path: PathBuf,
    avd: bool,
    exclusions: &Exclusions,
    cancellation: &Cancellation,
    mut report: impl FnMut(ScanProgress),
) -> (ScanProgress, ScanOutcome, Option<AvdSizes>) {
    let mut scan = Traversal {
        progress: ScanProgress::default(),
        hierarchy: None,
        identity: None,
        cleanup_identities: RecordedIdentities::new(),
        developer: None,
        avd: avd.then(AvdSizes::default),
        started: Instant::now(),
        last_report: Duration::ZERO,
        interval: PROGRESS_INTERVAL,
    };
    let result = if cancellation.is_cancelled() {
        Ok(())
    } else {
        walk(
            root,
            path,
            (!exclusions.is_empty()).then_some(exclusions),
            cancellation,
            &mut scan,
            &mut report,
        )
    };
    let outcome = if cancellation.is_cancelled() {
        ScanOutcome::Cancelled
    } else {
        result.map_or_else(ScanOutcome::Failed, |()| ScanOutcome::Completed)
    };
    (scan.progress, outcome, scan.avd)
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
