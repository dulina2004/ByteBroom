use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use slint::{CloseRequestResponse, ComponentHandle, Timer, TimerMode, VecModel};

use crate::{
    scanner::{self, Cancellation, ScanOutcome, ScanProgress, ScanRequest, ScanResult},
    storage::format_bytes,
};

use super::artifacts::{Browser, Catalog};
use super::{AppWindow, DirectoryDisplay, ScanDisplay, ScanPhase};

const UI_INTERVAL: Duration = Duration::from_millis(160);

#[derive(Default)]
pub(super) struct ScanController {
    job: RefCell<Option<Job>>,
    timer: Timer,
    closing: Cell<bool>,
    artifacts: Rc<Browser>,
}

enum Job {
    Picking(JoinHandle<Result<Option<PathBuf>, String>>),
    Scanning {
        worker: JoinHandle<(ScanResult, Catalog)>,
        cancellation: Arc<Cancellation>,
        progress: Arc<Mutex<ScanProgress>>,
        started: Instant,
    },
}

impl ScanController {
    pub(super) fn attach(window: &AppWindow) -> Rc<Self> {
        let controller = Rc::new(Self {
            artifacts: Browser::attach(window),
            ..Self::default()
        });

        let (scan, ui) = (Rc::clone(&controller), window.as_weak());
        window.on_scan_drive(move |letter| {
            if let (Some(window), [drive @ b'A'..=b'Z', b':']) =
                (ui.upgrade(), letter.as_str().as_bytes())
            {
                scan.start(&window, PathBuf::from(format!("{}:\\", char::from(*drive))));
            }
        });
        let (scan, ui) = (Rc::clone(&controller), window.as_weak());
        window.on_pick_scan_folder(move || {
            if let Some(window) = ui.upgrade() {
                scan.pick(&window);
            }
        });
        let (scan, ui) = (Rc::clone(&controller), window.as_weak());
        window.on_cancel_scan(move || {
            if let Some(window) = ui.upgrade() {
                scan.cancel(&window);
            }
        });
        let (scan, ui) = (Rc::clone(&controller), window.as_weak());
        window.window().on_close_requested(move || {
            if scan.job.borrow().is_none() {
                return CloseRequestResponse::HideWindow;
            }
            if let Some(window) = ui.upgrade() {
                match scan.job.borrow().as_ref() {
                    Some(Job::Scanning { .. }) => scan.closing.set(true),
                    _ => return CloseRequestResponse::KeepWindowShown,
                }
                scan.cancel(&window);
            }
            CloseRequestResponse::KeepWindowShown
        });
        controller
    }

    fn start(self: &Rc<Self>, window: &AppWindow, root: PathBuf) {
        if self.job.borrow().is_some() {
            return;
        }
        window.set_scan_root(root.to_string_lossy().as_ref().into());
        clear_stats(window);
        self.artifacts.replace(window, Catalog::default());
        window.set_scan_error("".into());
        let cancellation = Arc::new(Cancellation::default());
        let progress = Arc::new(Mutex::new(ScanProgress::default()));
        let (worker_cancellation, worker_progress) =
            (Arc::clone(&cancellation), Arc::clone(&progress));
        let started = Instant::now();
        let worker = thread::Builder::new()
            .name("filesystem-scan".into())
            .spawn(move || {
                let mut result =
                    scanner::scan(ScanRequest { root }, &worker_cancellation, |snapshot| {
                        if let Ok(mut latest) = worker_progress.lock() {
                            *latest = snapshot;
                        }
                    });
                let catalog = Catalog::new(std::mem::take(&mut result.developer));
                (result, catalog)
            });
        match worker {
            Ok(worker) => {
                window.set_scan_phase(ScanPhase::Scanning);
                *self.job.borrow_mut() = Some(Job::Scanning {
                    worker,
                    cancellation,
                    progress,
                    started,
                });
                self.start_timer(window);
            }
            Err(error) => show_error(window, format!("Could not start the scan worker. {error}")),
        }
    }

    fn pick(self: &Rc<Self>, window: &AppWindow) {
        if self.job.borrow().is_some() {
            return;
        }
        #[cfg(windows)]
        let owner = super::folder_picker::owner(window);
        #[cfg(not(windows))]
        let owner: Result<isize, String> =
            Err("The folder picker is currently available on Windows only.".into());
        let worker = owner.and_then(|owner| {
            thread::Builder::new()
                .name("folder-picker".into())
                .spawn(move || {
                    #[cfg(windows)]
                    return super::folder_picker::choose(owner);
                    #[cfg(not(windows))]
                    {
                        let _ = owner;
                        Ok(None)
                    }
                })
                .map_err(|error| error.to_string())
        });
        match worker {
            Ok(worker) => {
                window.set_picking_scan_folder(true);
                *self.job.borrow_mut() = Some(Job::Picking(worker));
                self.start_timer(window);
            }
            Err(error) => {
                self.artifacts.replace(window, Catalog::default());
                picker_error(window, format!("Could not open the folder picker. {error}"))
            }
        }
    }

    fn start_timer(self: &Rc<Self>, window: &AppWindow) {
        let (scan, ui) = (Rc::downgrade(self), window.as_weak());
        self.timer.start(TimerMode::Repeated, UI_INTERVAL, move || {
            if let (Some(scan), Some(window)) = (scan.upgrade(), ui.upgrade()) {
                scan.poll(&window);
            }
        });
    }

    fn cancel(&self, window: &AppWindow) {
        if let Some(Job::Scanning {
            cancellation,
            worker,
            ..
        }) = self.job.borrow().as_ref()
        {
            cancel_worker(cancellation, worker);
            window.set_scan_phase(ScanPhase::Cancelling);
        }
    }

    fn poll(self: &Rc<Self>, window: &AppWindow) {
        let mut job = self.job.borrow_mut();
        let finished = match job.as_ref() {
            Some(Job::Picking(worker)) => worker.is_finished(),
            Some(Job::Scanning {
                worker,
                cancellation,
                progress,
                started,
                ..
            }) => {
                let finished = worker.is_finished();
                if !finished && cancellation.is_cancelled() {
                    cancel_worker(cancellation, worker);
                }
                if !finished && let Ok(snapshot) = progress.try_lock() {
                    let progress = ScanProgress {
                        elapsed: started.elapsed(),
                        ..*snapshot
                    };
                    drop(snapshot);
                    window.set_scan_progress(display(progress));
                }
                finished
            }
            None => return,
        };
        if !finished {
            return;
        }
        let work = job.take().expect("a finished job exists");
        drop(job);
        self.timer.stop();
        // Join only after is_finished(), so the Slint event loop never waits on I/O.
        match work {
            Job::Picking(worker) => {
                window.set_picking_scan_folder(false);
                match worker.join() {
                    Ok(Ok(Some(root))) => self.start(window, root),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        self.artifacts.replace(window, Catalog::default());
                        picker_error(window, format!("Could not choose a folder. {error}"))
                    }
                    Err(_) => {
                        self.artifacts.replace(window, Catalog::default());
                        picker_error(window, "The folder picker closed unexpectedly.".into());
                    }
                }
            }
            Job::Scanning { worker, .. } => {
                match worker.join() {
                    Ok((result, catalog)) => {
                        self.artifacts.replace(window, catalog);
                        show_result(window, result);
                    }
                    Err(_) => show_error(window, "The scan worker stopped unexpectedly.".into()),
                }
                if self.closing.get() {
                    let _ = window.hide();
                }
            }
        }
    }

    pub(super) fn shutdown(&self) {
        self.timer.stop();
        if let Some(job) = self.job.borrow_mut().take() {
            match job {
                Job::Scanning {
                    cancellation,
                    worker,
                    ..
                } => {
                    cancel_worker(&cancellation, &worker);
                    let _ = worker.join();
                }
                Job::Picking(_) => {
                    // An unexpected event-loop shutdown exits the process. No scan is running.
                }
            }
        }
    }
}

fn cancel_worker(cancellation: &Cancellation, worker: &JoinHandle<(ScanResult, Catalog)>) {
    cancellation.cancel();
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::IO::CancelSynchronousIo;

        // SAFETY: the live JoinHandle owns the thread handle. Windows requests cancellation
        // without waiting for the filesystem to complete the currently pending operation.
        unsafe { CancelSynchronousIo(worker.as_raw_handle()) };
    }
    #[cfg(not(windows))]
    let _ = worker;
}

fn clear_stats(window: &AppWindow) {
    window.set_scan_progress(display(ScanProgress::default()));
    window.set_scan_directories(Default::default());
}

fn picker_error(window: &AppWindow, message: String) {
    window.set_scan_root("".into());
    clear_stats(window);
    show_error(window, message);
}

fn show_error(window: &AppWindow, message: String) {
    window.set_scan_error(message.into());
    window.set_scan_phase(ScanPhase::Failed);
}

fn show_result(window: &AppWindow, result: ScanResult) {
    window.set_scan_root(result.root.to_string_lossy().as_ref().into());
    window.set_scan_progress(display(result.progress));
    let directories = result
        .top_directories
        .iter()
        .map(|row| DirectoryDisplay {
            name: row.name.to_string_lossy().as_ref().into(),
            logical_size: format_bytes(row.logical_bytes).into(),
        })
        .collect::<Vec<_>>();
    window.set_scan_directories(Rc::new(VecModel::from(directories)).into());
    match result.outcome {
        ScanOutcome::Completed => window.set_scan_phase(ScanPhase::Completed),
        ScanOutcome::Cancelled => window.set_scan_phase(ScanPhase::Cancelled),
        ScanOutcome::Failed(error) => {
            show_error(window, format!("The scan could not finish. {error}"))
        }
    }
}

fn display(progress: ScanProgress) -> ScanDisplay {
    let seconds = progress.elapsed.as_secs();
    ScanDisplay {
        logical_size: format_bytes(progress.logical_bytes).into(),
        files: progress.files.to_string().into(),
        directories: progress.directories.to_string().into(),
        inaccessible: progress.inaccessible.to_string().into(),
        skipped_links: progress.skipped_links.to_string().into(),
        projects: progress.projects.to_string().into(),
        artifact_candidates: progress.artifact_candidates.to_string().into(),
        elapsed: format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        )
        .into(),
    }
}
