mod comparison;
mod treemap;

pub(crate) use comparison::compare;
pub(crate) use treemap::layout;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::{
    detectors::{Measurement, Results},
    scanner::{ScanOutcome, ScanResult, ScopeIdentity},
};

pub(crate) const ITEM_LIMIT: usize = 512;

#[derive(Debug)]
pub(crate) struct Folder {
    pub(crate) name: OsString,
    pub(crate) measurement: Measurement,
    pub(crate) direct_bytes: u64,
    pub(crate) omitted_bytes: u64,
    pub(crate) omitted_count: u64,
    pub(crate) expanded: bool,
    pub(crate) complete: bool,
    pub(crate) children: Vec<Folder>,
}

#[derive(Default, Debug)]
pub(crate) struct Dataset {
    pub(crate) rows: Vec<Item>,
    pub(crate) count: usize,
    pub(crate) omitted: Option<u64>,
    pub(crate) protected_bytes: Option<u64>,
    pub(crate) partial: bool,
}

impl Dataset {
    fn new(mut rows: Vec<Item>, partial: bool) -> Self {
        rows.sort_unstable_by(by_size);
        let count = rows.len();
        let omitted = sum(rows.iter().skip(ITEM_LIMIT).map(|item| item.bytes));
        let protected_bytes = sum(rows
            .iter()
            .filter(|item| item.protected)
            .map(|item| item.bytes));
        rows.truncate(ITEM_LIMIT);
        Self {
            rows,
            count,
            omitted,
            protected_bytes,
            partial,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) name: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) group: String,
    pub(crate) category: &'static str,
    pub(crate) status: &'static str,
    pub(crate) risk: &'static str,
    pub(crate) bytes: Option<u64>,
    pub(crate) description: String,
    pub(crate) child: Option<usize>,
    pub(crate) protected: bool,
}

impl Item {
    pub(crate) fn contains(&self, query: &str) -> bool {
        query.is_empty()
            || [&*self.name, &*self.group, self.category, self.risk]
                .iter()
                .any(|value| value.to_lowercase().contains(query))
            || self
                .path
                .as_ref()
                .is_some_and(|path| path.to_string_lossy().to_lowercase().contains(query))
    }

    pub(crate) fn synthetic(
        name: &str,
        category: &'static str,
        bytes: u64,
        description: String,
    ) -> Self {
        Self {
            name: name.into(),
            path: None,
            group: String::new(),
            category,
            status: "Measured",
            risk: "",
            bytes: Some(bytes),
            description,
            child: None,
            protected: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Catalog {
    pub(crate) root: PathBuf,
    pub(crate) hierarchy: Folder,
    pub(crate) artifacts: Dataset,
    pub(crate) projects: Dataset,
    pub(crate) ecosystems: Dataset,
    pub(crate) notice: String,
    identity: Option<ScopeIdentity>,
}

impl Catalog {
    pub(crate) fn from_scan(scan: &mut ScanResult) -> Option<Self> {
        let hierarchy = scan.hierarchy.take()?;
        let notice = match scan.outcome {
            ScanOutcome::Cancelled => "Scan cancelled. Measurements are partial.".to_owned(),
            ScanOutcome::Failed(_) => {
                "Scan stopped at an error. Measurements are partial.".to_owned()
            }
            ScanOutcome::Completed if scan.progress.inaccessible != 0 => format!(
                "Partial measurement · {} inaccessible entries.",
                scan.progress.inaccessible
            ),
            ScanOutcome::Completed => String::new(),
        };
        let (artifacts, projects, ecosystems) = project_data(&scan.developer);
        Some(Self {
            root: scan.root.clone(),
            hierarchy,
            artifacts,
            projects,
            ecosystems,
            notice,
            identity: scan.identity.take(),
        })
    }

    pub(crate) fn folder(&self, route: &[usize]) -> Option<(&Folder, PathBuf)> {
        let mut folder = &self.hierarchy;
        let mut path = self.root.clone();
        for &index in route {
            folder = folder.children.get(index)?;
            path.push(&folder.name);
        }
        Some((folder, path))
    }
}

impl Folder {
    pub(crate) fn rows(&self, path: &Path) -> Vec<Item> {
        let mut rows = Vec::with_capacity(self.children.len() + 2);
        for (index, child) in self.children.iter().enumerate() {
            rows.push(Item {
                name: child.name.to_string_lossy().into_owned(),
                path: Some(path.join(&child.name)),
                group: String::new(),
                category: "Directory",
                status: if child.complete {
                    "Measured"
                } else {
                    "Partial"
                },
                risk: "",
                bytes: Some(child.measurement.logical_bytes),
                description: format!(
                    "{} files · {} directories · {} inaccessible entries",
                    child.measurement.files,
                    child.measurement.directories,
                    child.measurement.inaccessible
                ),
                child: Some(index),
                protected: false,
            });
        }
        if self.expanded {
            if self.direct_bytes != 0 {
                rows.push(Item::synthetic(
                    "Files in this folder",
                    "Direct files",
                    self.direct_bytes,
                    "Logical size of files directly inside this folder.".into(),
                ));
            }
            if self.omitted_count != 0 {
                rows.push(Item::synthetic(
                    "Other measured items",
                    "Omitted directories",
                    self.omitted_bytes,
                    format!(
                        "{} measured child directories beyond the retained list.",
                        self.omitted_count
                    ),
                ));
            }
        }
        rows
    }
}

fn project_data(results: &Results) -> (Dataset, Dataset, Dataset) {
    let mut by_project = vec![None; results.projects.len()];
    let mut groups = BTreeMap::<_, (usize, usize, u64, bool)>::new();
    for project in &results.projects {
        for ecosystem in &project.ecosystems {
            groups.entry(*ecosystem).or_default().0 += 1;
        }
    }
    let artifacts = results
        .artifacts
        .iter()
        .filter_map(|artifact| {
            let project = results.projects.get(artifact.project.0)?;
            let bytes = artifact.measurement.logical_bytes;
            let entry = &mut by_project[artifact.project.0];
            if entry.as_ref().is_none_or(|(size, _)| *size < bytes) {
                *entry = Some((bytes, artifact.name().into_owned()));
            }
            let group = groups.entry(artifact.ecosystem).or_default();
            group.1 += 1;
            group.2 = group.2.saturating_add(bytes);
            group.3 |= !artifact.measured_completely;
            Some(Item {
                name: artifact.name().into_owned(),
                path: Some(artifact.path.clone()),
                group: format!("{} · {}", artifact.ecosystem.label(), project.name()),
                category: artifact.category.label(),
                status: if artifact.measured_completely {
                    "Measured"
                } else {
                    "Partial"
                },
                risk: artifact.review.label(),
                bytes: Some(bytes),
                child: None,
                protected: false,
                description: format!(
                    "{} Evidence: {} and {}. {}",
                    artifact.description,
                    artifact.evidence.project_marker,
                    artifact.evidence.artifact_marker,
                    artifact.warning
                ),
            })
        })
        .collect();
    let projects = results
        .projects
        .iter()
        .zip(by_project)
        .map(|(project, largest)| {
            let description = format!(
                "{} detected artifacts. Largest: {}. Evidence: {}.",
                project.artifact_count,
                largest
                    .as_ref()
                    .map_or("None detected", |(_, name)| name.as_str()),
                project.tools.join(", ")
            );
            Item {
                name: project.name().into_owned(),
                path: Some(project.root.clone()),
                group: project
                    .ecosystems
                    .iter()
                    .map(|eco| eco.label())
                    .collect::<Vec<_>>()
                    .join(" · "),
                category: "Detected project artifacts",
                status: if project.incomplete || results.partial {
                    "Partial"
                } else {
                    "Measured"
                },
                risk: "",
                bytes: Some(project.logical_bytes),
                description,
                child: None,
                protected: false,
            }
        })
        .collect();
    let ecosystems = groups
        .into_iter()
        .map(|(ecosystem, (projects, artifacts, bytes, partial))| {
            let mut item = Item::synthetic(
                ecosystem.label(),
                "Project ecosystem",
                bytes,
                format!("{projects} projects associated · {artifacts} detected artifacts"),
            );
            if results.partial || partial {
                item.status = "Partial";
            }
            item
        })
        .collect();
    (
        Dataset::new(artifacts, results.partial),
        Dataset::new(projects, results.partial),
        Dataset::new(ecosystems, results.partial),
    )
}

#[cfg(windows)]
pub(crate) fn global_data(inventory: &crate::global_caches::Inventory) -> [Dataset; 6] {
    use crate::global_caches::{Kind, Status};

    let partial = inventory.cancelled
        || inventory.truncated
        || inventory.resources.iter().any(|resource| {
            matches!(
                resource.status,
                Status::Partial | Status::Cancelled | Status::Unavailable
            )
        });
    let rows = Dataset::new(
        inventory
            .resources
            .iter()
            .map(|resource| Item {
                name: resource.name.clone(),
                path: Some(resource.path.clone()),
                group: format!(
                    "{} · {} · {}:",
                    resource.tool.group().label(),
                    resource.tool.name(),
                    resource.drive
                ),
                category: resource.kind.label(),
                status: resource.status.label(),
                risk: resource.risk(),
                bytes: resource.measured.map(|value| value.logical_bytes),
                child: None,
                protected: resource.kind == Kind::Protected,
                description: format!(
                    "{} {} {}. {}. {} {} {}",
                    resource.description,
                    resource.effects().0,
                    resource.source.label(),
                    resource.confidence.label(),
                    resource.effects().1,
                    resource.evidence,
                    resource.note
                ),
            })
            .collect(),
        partial,
    );
    let grouped = |axis: usize| {
        let mut groups = BTreeMap::<String, (usize, Option<u64>, Option<u64>, bool)>::new();
        for resource in &inventory.resources {
            let key = match axis {
                0 => resource.tool.group().label().into(),
                1 => resource.tool.name().into(),
                2 => format!("{}:", resource.drive),
                3 => resource.kind.label().into(),
                _ => resource.risk().into(),
            };
            let group = groups.entry(key).or_default();
            group.0 += 1;
            if let Some(measurement) = resource.measured {
                group.1 = sum([group.1, Some(measurement.logical_bytes)].into_iter());
                if resource.kind == Kind::Protected {
                    group.2 = sum([group.2, Some(measurement.logical_bytes)].into_iter());
                }
            }
            group.3 |= resource.status != Status::Measured;
        }
        let rows = groups
            .into_iter()
            .map(|(name, (count, bytes, protected, partial))| {
                let description = format!(
                    "{count} locations · Protected logical size {}.",
                    protected
                        .map(crate::storage::format_bytes)
                        .unwrap_or_else(|| "Unknown".into())
                );
                let mut item = Item::synthetic(
                    &name,
                    "Global storage group",
                    bytes.unwrap_or_default(),
                    description,
                );
                item.bytes = bytes;
                item.status = if bytes.is_none() {
                    "Unmeasured"
                } else if partial {
                    "Partial"
                } else {
                    "Measured"
                };
                item
            })
            .collect();
        let mut dataset = Dataset::new(rows, partial);
        dataset.protected_bytes = rows_protected(inventory);
        dataset
    };
    [
        rows,
        grouped(0),
        grouped(1),
        grouped(2),
        grouped(3),
        grouped(4),
    ]
}

#[cfg(windows)]
fn rows_protected(inventory: &crate::global_caches::Inventory) -> Option<u64> {
    sum(inventory
        .resources
        .iter()
        .filter(|item| item.kind == crate::global_caches::Kind::Protected)
        .map(|item| item.measured.map(|value| value.logical_bytes)))
}

pub(crate) fn sum(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values.flatten().reduce(u64::saturating_add)
}

fn by_size(a: &Item, b: &Item) -> std::cmp::Ordering {
    b.bytes
        .cmp(&a.bytes)
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.path.cmp(&b.path))
}

pub(crate) fn matching(
    items: &[Item],
    query: &str,
    category: &str,
    minimum: Option<u64>,
    order: i32,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut indices = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            (category.is_empty() || [item.category, item.status, item.risk].contains(&category))
                && minimum
                    .is_none_or(|threshold| item.bytes.is_some_and(|bytes| bytes >= threshold))
                && item.contains(&query)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    indices.sort_unstable_by(|&a, &b| {
        let (a, b) = (&items[a], &items[b]);
        let name = || a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path));
        match order {
            1 => b
                .bytes
                .is_some()
                .cmp(&a.bytes.is_some())
                .then_with(|| a.bytes.cmp(&b.bytes))
                .then_with(name),
            2 => name(),
            _ => by_size(a, b),
        }
    });
    indices
}
