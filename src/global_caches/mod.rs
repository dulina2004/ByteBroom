mod model;
mod providers;
#[cfg(test)]
mod tests;
mod windows;

pub(crate) use model::{Inventory, Kind, Progress, Resource, Status, Tool};
pub(crate) use providers::CustomRoot;
pub(crate) use windows::drive as path_drive;

use std::{
    collections::{BTreeSet, HashSet},
    ffi::OsString,
    io,
    os::windows::ffi::OsStringExt,
    time::Instant,
};

use crate::{
    detectors::{Confidence, Measurement},
    scanner::{self, Cancellation, ScanOutcome, windows::Entry},
    windows_fs::ErrorMode,
};
use model::{Identity, Source};
use providers::{Definition, Environment, Layout, Location, Plan};

const MAX_LOCATIONS: usize = 512;
const MAX_NOTICES: usize = 64;
const MAX_CHILD_ENTRIES: usize = 1024;
pub(crate) const MAX_CUSTOM_ROOTS: usize = 16;

pub(crate) fn discover(
    selected: Option<Tool>,
    custom: &[CustomRoot],
    stop: &Cancellation,
    report: impl FnMut(Progress),
) -> Inventory {
    if stop.is_cancelled() {
        return Inventory {
            cancelled: true,
            ..Inventory::default()
        };
    }
    discover_with(&windows::environment(), selected, custom, stop, report)
}

#[cfg(test)]
pub(crate) fn discover_fixture(tool: Tool, custom: &[CustomRoot]) -> Inventory {
    discover_with(
        &Environment::default(),
        Some(tool),
        custom,
        &Cancellation::default(),
        |_| (),
    )
}

fn discover_with(
    env: &Environment,
    selected: Option<Tool>,
    custom: &[CustomRoot],
    stop: &Cancellation,
    mut report: impl FnMut(Progress),
) -> Inventory {
    let mut state = Discovery {
        inventory: Inventory::default(),
        identities: HashSet::new(),
        tools: BTreeSet::new(),
        stop,
    };
    let _error_mode = match ErrorMode::suppress_dialogs() {
        Ok(mode) => mode,
        Err(error) => {
            state.notice(format!(
                "Windows metadata discovery could not start ({error})."
            ));
            return state.inventory;
        }
    };
    let started = Instant::now();
    let known = [
        &env.home,
        &env.local,
        &env.roaming,
        &env.program_data,
        &env.temp,
    ];
    let protected = known
        .into_iter()
        .flatten()
        .take_while(|_| !stop.is_cancelled())
        .filter_map(|path| {
            windows::open(path, stop)
                .ok()
                .flatten()
                .map(|(_, id)| id.canonical)
        })
        .collect::<Vec<_>>();
    let mut verified = std::collections::HashMap::new();
    for plan in providers::plans(env, &custom[..custom.len().min(MAX_CUSTOM_ROOTS)], selected) {
        if stop.is_cancelled() {
            break;
        }
        if state.inventory.resources.len() == MAX_LOCATIONS {
            state.inventory.truncated = true;
            break;
        }
        let base = &plan.location.base;
        // Repeated wrapper/SDK subdirectories share their base validation.
        let valid = *verified
            .entry((plan.tool, base.clone()))
            .or_insert_with(|| {
                let basic = windows::drive(base).is_some();
                let local = if basic && plan.location.source != Source::Default {
                    windows::open(base, stop)
                        .ok()
                        .flatten()
                        .is_some_and(|(_, id)| {
                            !protected
                                .iter()
                                .any(|blocked| blocked.starts_with(&id.canonical))
                        })
                } else {
                    basic
                };
                if !local && !stop.is_cancelled() {
                    state.notice(format!(
                        "{} · {} could not be verified as a narrow local directory.",
                        plan.tool.name(),
                        plan.location.source.label()
                    ));
                }
                local
            });
        if valid {
            state.expand(plan);
        }
        report(Progress {
            tools: state.tools.len(),
            locations: state.inventory.resources.len(),
            elapsed: started.elapsed(),
            ..Progress::default()
        });
    }
    state.inventory.cancelled = stop.is_cancelled();
    state.inventory
}

struct Discovery<'a> {
    inventory: Inventory,
    identities: HashSet<(u64, [u8; 16])>,
    tools: BTreeSet<Tool>,
    stop: &'a Cancellation,
}

impl Discovery<'_> {
    fn notice(&mut self, message: String) {
        if self.inventory.notices.len() < MAX_NOTICES && !self.inventory.notices.contains(&message)
        {
            self.inventory.notices.push(message);
        }
    }

    fn expand(&mut self, plan: Plan) {
        let mut directory = match windows::open(&plan.location.path, self.stop) {
            Ok(Some((directory, identity))) => {
                if matches!(plan.layout, Layout::Directory) {
                    self.push(
                        &plan,
                        String::from(plan.definition.name),
                        Some(identity),
                        Status::Ready,
                        String::new(),
                    );
                    return;
                }
                directory
            }
            Ok(None) => return,
            Err(error) => {
                self.failed(&plan, error);
                return;
            }
        };
        let mut pnpm_layout = false;
        let mut pnpm_versions = false;
        for _ in 0..MAX_CHILD_ENTRIES {
            if self.stop.is_cancelled() {
                return;
            }
            let name = match directory.next(false, false) {
                Ok(Some(Entry::Directory(name))) => OsString::from_wide(&name),
                Ok(Some(_)) => continue,
                Ok(None) => {
                    if matches!(plan.layout, Layout::PnpmStore) && pnpm_layout && !pnpm_versions {
                        self.leaf(
                            &plan,
                            plan.location.clone(),
                            plan.definition,
                            String::from(plan.definition.name),
                        );
                    }
                    return;
                }
                Err(error) => {
                    self.notice(format!(
                        "{} · a storage layout could only be partly inspected ({error}).",
                        plan.tool.name()
                    ));
                    return;
                }
            };
            let text = name.to_string_lossy();
            let lower = text.to_ascii_lowercase();
            if matches!(plan.layout, Layout::PnpmStore)
                && matches!(lower.as_str(), "files" | "index")
            {
                pnpm_layout = true;
                continue;
            }
            let mut def = plan.definition;
            let known = match plan.layout {
                Layout::Directory => false,
                Layout::PnpmStore => lower.strip_prefix('v').is_some_and(digits),
                Layout::GradleCaches => {
                    if lower == "modules-2" {
                        def = Definition {
                            name: "Dependency cache",
                            kind: Kind::Dependencies,
                            description: "Gradle dependency artifacts and metadata shared across builds.",
                        };
                        true
                    } else {
                        lower.starts_with("build-cache-") || lower.starts_with("transforms-")
                    }
                }
                Layout::AndroidApi => lower.strip_prefix("android-").is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                }),
                Layout::Versions => {
                    lower.split('.').next().is_some_and(digits)
                        && lower
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                }
                Layout::Avd => lower.ends_with(".avd") && lower.len() > 4,
                Layout::JetBrains => ide_version(&lower),
            };
            if !known {
                continue;
            }
            let location = Location {
                path: plan.location.path.join(&name),
                ..plan.location.clone()
            };
            if matches!(plan.layout, Layout::JetBrains) {
                for (child, label, kind, description) in [
                    (
                        "caches",
                        "Index and system caches",
                        Kind::Cache,
                        "JetBrains caches and index data. Indexes may take time to regenerate.",
                    ),
                    (
                        "log",
                        "IDE logs",
                        Kind::State,
                        "IDE diagnostic logs that can contain useful investigation history.",
                    ),
                    (
                        "LocalHistory",
                        "Local History",
                        Kind::Protected,
                        "Local History can contain edits that were never committed to version control.",
                    ),
                ] {
                    let location = Location {
                        path: location.path.join(child),
                        ..location.clone()
                    };
                    self.leaf(
                        &plan,
                        location,
                        Definition {
                            name: label,
                            kind,
                            description,
                        },
                        format!("{text} · {label}"),
                    );
                }
            } else {
                pnpm_versions |= matches!(plan.layout, Layout::PnpmStore);
                self.leaf(&plan, location, def, format!("{} · {text}", def.name));
            }
            if self.inventory.resources.len() == MAX_LOCATIONS {
                self.inventory.truncated = true;
                return;
            }
        }
        self.inventory.truncated = true;
    }

    fn leaf(&mut self, plan: &Plan, location: Location, definition: Definition, name: String) {
        let leaf = Plan {
            tool: plan.tool,
            location,
            definition,
            layout: plan.layout,
        };
        match windows::open(&leaf.location.path, self.stop) {
            Ok(Some((_, identity))) => {
                self.push(&leaf, name, Some(identity), Status::Ready, String::new())
            }
            Ok(None) => {}
            Err(error) => self.failed(&leaf, error),
        }
    }

    fn failed(&mut self, plan: &Plan, error: io::Error) {
        if self.stop.is_cancelled() || error.kind() == io::ErrorKind::NotFound {
            return;
        }
        let status = if matches!(
            error.kind(),
            io::ErrorKind::InvalidInput | io::ErrorKind::NotADirectory
        ) {
            Status::Skipped
        } else {
            Status::Unavailable
        };
        self.push(
            plan,
            String::from(plan.definition.name),
            None,
            status,
            error.to_string(),
        );
    }

    fn push(
        &mut self,
        plan: &Plan,
        name: String,
        identity: Option<Identity>,
        status: Status,
        note: String,
    ) {
        if self.inventory.resources.len() >= MAX_LOCATIONS {
            self.inventory.truncated = true;
            return;
        }
        let Some(drive) = windows::drive(&plan.location.path) else {
            return;
        };
        if let Some(identity) = &identity
            && !self.identities.insert((identity.volume, identity.file))
        {
            return;
        }
        self.tools.insert(plan.tool);
        let source = plan.location.source;
        let reviewed_path = source != Source::Default;
        self.inventory.resources.push(Resource {
            id: self.inventory.resources.len(), tool: plan.tool, name, kind: plan.definition.kind,
            path: plan.location.path.clone(), drive, source,
            confidence: if reviewed_path || status != Status::Ready { Confidence::Contextual } else { Confidence::High },
            description: plan.definition.description,
            evidence: if status != Status::Ready {
                "Expected tool location. Windows could not verify the directory."
            } else if matches!(plan.layout, Layout::Directory) {
                "Exact tool location or documented child directory; verified with local directory metadata."
            } else { "Recognized tool-specific directory name and layout; verified with local directory metadata." },
            status, measured: None, skipped_links: 0, avd: None, note,
            selected: status == Status::Ready && !reviewed_path,
            identity,
        });
    }
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn ide_version(value: &str) -> bool {
    [
        "intellijidea",
        "ideau",
        "ideac",
        "androidstudio",
        "pycharm",
        "webstorm",
        "clion",
        "rider",
        "rustrover",
        "goland",
        "phpstorm",
        "rubymine",
        "datagrip",
        "dataspell",
    ]
    .into_iter()
    .any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|suffix| {
            suffix
                .trim_start_matches(['-', 'c', 'e', 'p'])
                .split('.')
                .next()
                .is_some_and(digits)
        })
    })
}

pub(crate) fn measure(
    mut inventory: Inventory,
    stop: &Cancellation,
    mut report: impl FnMut(Progress),
) -> Inventory {
    inventory.cancelled = false;
    let mut selected = Vec::new();
    for (index, item) in inventory.resources.iter_mut().enumerate() {
        if item.identity.is_none() {
            continue;
        }
        item.measured = None;
        item.skipped_links = 0;
        item.avd = None;
        if item.selected {
            item.status = Status::Ready;
            item.note.clear();
            selected.push(index);
        } else {
            item.status = Status::Skipped;
            item.note = "Outside the selected measurement scope.".into();
        }
    }
    let started = Instant::now();
    let mut progress = Progress {
        tools: inventory
            .resources
            .iter()
            .map(|item| item.tool)
            .collect::<BTreeSet<_>>()
            .len(),
        locations: selected.len(),
        ..Progress::default()
    };
    let _error_mode = match ErrorMode::suppress_dialogs() {
        Ok(mode) => mode,
        Err(error) => {
            for index in selected {
                inventory.resources[index].status = Status::Unavailable;
                inventory.resources[index].note = error.to_string();
            }
            return inventory;
        }
    };
    let boundaries: Vec<_> = inventory
        .resources
        .iter()
        .filter_map(|item| item.identity.as_ref().map(|id| id.canonical.clone()))
        .collect();
    for index in selected {
        let item = &mut inventory.resources[index];
        if stop.is_cancelled() {
            item.status = Status::Cancelled;
            item.note = "Measurement did not begin.".into();
            continue;
        }
        let identity = item
            .identity
            .as_ref()
            .expect("only validated locations are queued");
        // Each listed child owns its subtree, even when it is excluded from measurement.
        let mut exclusions = scanner::Exclusions::default();
        for candidate in &boundaries {
            if candidate != &identity.canonical
                && let Ok(relative) = candidate.strip_prefix(&identity.canonical)
            {
                exclusions.insert(relative);
            }
        }
        match windows::open(&item.path, stop) {
            Ok(Some((handle, current))) if current == *identity => {
                let (stats, outcome, avd) = scanner::measure(
                    handle,
                    item.path.clone(),
                    item.tool == Tool::AndroidAvd,
                    &exclusions,
                    stop,
                    |stats| {
                        report(Progress {
                            files: progress.files.saturating_add(stats.files),
                            elapsed: started.elapsed(),
                            ..progress
                        });
                    },
                );
                let present = stats.directories != 0 && !matches!(outcome, ScanOutcome::Failed(_))
                    || stats.files != 0
                    || stats.directories > 1;
                item.measured = present.then_some(Measurement {
                    logical_bytes: stats.logical_bytes,
                    files: stats.files,
                    directories: stats.directories,
                    inaccessible: stats.inaccessible,
                });
                item.skipped_links = stats.skipped_links;
                item.avd = avd.filter(|_| present);
                item.status = match outcome {
                    ScanOutcome::Completed if stats.inaccessible == 0 => Status::Measured,
                    ScanOutcome::Completed => {
                        item.note = "Some entries were inaccessible.".into();
                        Status::Partial
                    }
                    ScanOutcome::Failed(error) => {
                        item.note = error.to_string();
                        if present {
                            Status::Partial
                        } else {
                            Status::Unavailable
                        }
                    }
                    ScanOutcome::Cancelled => {
                        item.note = "Cancelled during measurement.".into();
                        Status::Cancelled
                    }
                };
                if !exclusions.is_empty() {
                    if !item.note.is_empty() {
                        item.note.push(' ');
                    }
                    item.note.push_str(
                        "Separately listed child locations are outside this row’s measurement.",
                    );
                }
                progress.files = progress.files.saturating_add(stats.files);
            }
            Ok(None) => {
                item.status = Status::Cancelled;
            }
            Ok(Some(_)) => {
                item.status = Status::Unavailable;
                item.note =
                    "The directory changed after discovery. Discover locations again.".into();
            }
            Err(_) if stop.is_cancelled() => {
                item.status = Status::Cancelled;
                item.note = "Cancelled before measurement.".into();
            }
            Err(error) => {
                item.status = Status::Unavailable;
                item.note = error.to_string();
            }
        }
        progress.processed += 1;
        progress.elapsed = started.elapsed();
        report(progress);
    }
    inventory.cancelled = stop.is_cancelled();
    inventory
}
