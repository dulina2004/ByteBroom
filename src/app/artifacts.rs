use std::{cell::RefCell, rc::Rc};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{
    detectors::{Artifact, Category, Ecosystem, Project, Results, Review},
    storage::format_bytes,
};

use super::{AppWindow, ArtifactDetails, ArtifactFilter, ArtifactRow, ArtifactSummary};

const PAGE_SIZE: usize = 12;
const MINIMUM_SIZES: [u64; 3] = [1 << 20, 100 << 20, 1 << 30];

// Prepared on the scan worker. Store search keys only for retained results.
#[derive(Default)]
pub(super) struct Catalog {
    results: Results,
    artifact_keys: Vec<String>,
    project_keys: Vec<String>,
    artifact_names: Vec<usize>,
    artifact_by_id: Vec<usize>,
    project_names: Vec<usize>,
    project_sizes: Vec<usize>,
}

impl Catalog {
    pub(super) fn new(results: Results) -> Self {
        let artifact_keys = results
            .artifacts
            .iter()
            .map(|item| item.path.to_string_lossy().to_lowercase())
            .collect();
        let project_keys = results
            .projects
            .iter()
            .map(|item| item.root.to_string_lossy().to_lowercase())
            .collect();
        let mut artifact_names = (0..results.artifacts.len()).collect::<Vec<_>>();
        let mut artifact_by_id = vec![0; results.artifacts.len()];
        for (index, entry) in results.artifacts.iter().enumerate() {
            artifact_by_id[entry.id.0] = index;
        }
        artifact_names.sort_unstable_by(|&a, &b| {
            results.artifacts[a]
                .path
                .file_name()
                .cmp(&results.artifacts[b].path.file_name())
                .then(a.cmp(&b))
        });
        let mut project_names = (0..results.projects.len()).collect::<Vec<_>>();
        project_names.sort_unstable_by(|&a, &b| {
            results.projects[a]
                .root
                .file_name()
                .cmp(&results.projects[b].root.file_name())
                .then(a.cmp(&b))
        });
        let mut project_sizes = project_names.clone();
        project_sizes
            .sort_by_key(|&index| std::cmp::Reverse(results.projects[index].logical_bytes));
        Self {
            results,
            artifact_keys,
            project_keys,
            artifact_names,
            artifact_by_id,
            project_names,
            project_sizes,
        }
    }
}

#[derive(Default)]
pub(super) struct Browser(RefCell<View>);

#[derive(Default)]
struct View {
    catalog: Catalog,
    filters: ArtifactFilter,
    project: Option<usize>,
    matching: Vec<usize>,
    page: usize,
}

impl Browser {
    pub(super) fn attach(window: &AppWindow) -> Rc<Self> {
        let browser = Rc::new(Self::default());
        window.set_artifact_ecosystems(options(
            "All ecosystems",
            Ecosystem::ALL.map(Ecosystem::label),
        ));
        window.set_artifact_categories(options(
            "All categories",
            Category::ALL.map(Category::label),
        ));
        window.set_artifact_reviews(options(
            "All classifications",
            Review::ALL.map(Review::label),
        ));
        let (data, ui) = (Rc::clone(&browser), window.as_weak());
        window.on_filter_artifacts(move |filters| {
            if let Some(window) = ui.upgrade() {
                let mut view = data.0.borrow_mut();
                if filters.grouped {
                    view.project = None;
                }
                view.filters = filters;
                view.apply(&window);
            }
        });
        let (data, ui) = (Rc::clone(&browser), window.as_weak());
        window.on_artifact_turn_page(move |delta| {
            if let Some(window) = ui.upgrade() {
                let mut view = data.0.borrow_mut();
                let pages = view.matching.len().div_ceil(PAGE_SIZE);
                view.page = view
                    .page
                    .saturating_add_signed(delta.clamp(-1, 1) as isize)
                    .min(pages.saturating_sub(1));
                view.render(&window);
            }
        });
        let (data, ui) = (Rc::clone(&browser), window.as_weak());
        window.on_artifact_select(move |index| {
            if let Some(window) = ui.upgrade() {
                let mut view = data.0.borrow_mut();
                let Ok(index) = usize::try_from(index) else {
                    window.set_artifact_details(ArtifactDetails::default());
                    return;
                };
                if view.filters.grouped && view.catalog.results.projects.get(index).is_some() {
                    view.project = Some(index);
                    view.filters.grouped = false;
                    view.apply(&window);
                } else if !view.filters.grouped
                    && let Some(&position) = view.catalog.artifact_by_id.get(index)
                {
                    let entry = &view.catalog.results.artifacts[position];
                    window.set_artifact_details(details(
                        index,
                        entry,
                        &view.catalog.results.projects[entry.project.0],
                    ));
                }
            }
        });
        let (data, ui) = (Rc::clone(&browser), window.as_weak());
        window.on_artifact_clear_project(move || {
            if let Some(window) = ui.upgrade() {
                let mut view = data.0.borrow_mut();
                view.project = None;
                view.filters.grouped = true;
                view.apply(&window);
            }
        });
        browser
    }

    pub(super) fn replace(&self, window: &AppWindow, catalog: Catalog) {
        let mut view = self.0.borrow_mut();
        let result = &catalog.results;
        window.set_artifact_summary(ArtifactSummary {
            projects: result.projects.len().to_string().into(),
            artifacts: result.artifacts.len().to_string().into(),
            logical_size: format_bytes(result.logical_bytes).into(),
            ecosystems: result.ecosystem_count.to_string().into(),
        });
        window.set_artifact_partial(result.partial);
        window.set_artifact_truncated(result.truncated);
        *view = View {
            catalog,
            ..View::default()
        };
        view.apply(window);
    }
}

impl View {
    fn apply(&mut self, window: &AppWindow) {
        let query = self.filters.query.trim().to_lowercase();
        let ecosystem = choice(self.filters.ecosystem, &Ecosystem::ALL);
        let category = choice(self.filters.category, &Category::ALL);
        let review = choice(self.filters.review, &Review::ALL);
        let minimum = choice(self.filters.minimum_size, &MINIMUM_SIZES);
        let result = &self.catalog.results;
        let records = result
            .artifacts
            .iter()
            .enumerate()
            .filter(|&(index, item)| {
                self.project.is_none_or(|project| item.project.0 == project)
                    && ecosystem.is_none_or(|value| item.ecosystem == value)
                    && category.is_none_or(|value| item.category == value)
                    && review.is_none_or(|value| item.review == value)
                    && minimum.is_none_or(|value| item.measurement.logical_bytes >= value)
                    && (self.catalog.artifact_keys[index].contains(&query)
                        || self.catalog.project_keys[item.project.0].contains(&query))
            });
        if self.filters.grouped {
            let mut matches = vec![false; result.projects.len()];
            for (_, item) in records {
                matches[item.project.0] = true;
            }
            let order = if self.filters.sort == 2 {
                &self.catalog.project_names
            } else {
                &self.catalog.project_sizes
            };
            let mut projects = order
                .iter()
                .copied()
                .filter(|&index| {
                    matches[index]
                        || (category.is_none()
                            && review.is_none()
                            && minimum.is_none()
                            && ecosystem.is_none_or(|value| {
                                result.projects[index].ecosystems.contains(&value)
                            })
                            && self.catalog.project_keys[index].contains(&query))
                })
                .collect::<Vec<_>>();
            if self.filters.sort == 1 {
                projects.reverse();
            }
            self.matching = projects;
        } else {
            let mut matches = records.map(|(index, _)| index).collect::<Vec<_>>();
            match self.filters.sort {
                1 => matches.reverse(),
                2 => {
                    // Precomputed order keeps text edits linear in the retained result count.
                    let mut selected = vec![false; result.artifacts.len()];
                    for index in matches {
                        selected[index] = true;
                    }
                    matches = self
                        .catalog
                        .artifact_names
                        .iter()
                        .copied()
                        .filter(|&index| selected[index])
                        .collect();
                }
                _ => {}
            }
            self.matching = matches;
        }
        self.page = 0;
        window.set_artifact_filter(self.filters.clone());
        window.set_artifact_project(self.project.map_or_else(SharedString::default, |index| {
            result.projects[index].name().as_ref().into()
        }));
        self.render(window);
    }

    fn render(&self, window: &AppWindow) {
        let start = self.page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.matching.len());
        let results = &self.catalog.results;
        let rows = self.matching[start..end]
            .iter()
            .map(|&index| {
                if self.filters.grouped {
                    project_row(index, &results.projects[index])
                } else {
                    let item = &results.artifacts[index];
                    artifact_row(item, &results.projects[item.project.0])
                }
            })
            .collect::<Vec<_>>();
        window.set_artifact_rows(Rc::new(VecModel::from(rows)).into());
        window.set_artifact_details(ArtifactDetails::default());
        window.set_artifact_page(if end == 0 {
            "0 results".into()
        } else {
            format!("{}–{end} of {}", start + 1, self.matching.len()).into()
        });
        window.set_artifact_previous(start != 0);
        window.set_artifact_next(end < self.matching.len());
    }
}

fn options(title: &str, labels: impl IntoIterator<Item = &'static str>) -> ModelRc<SharedString> {
    let values = std::iter::once(SharedString::from(title))
        .chain(labels.into_iter().map(SharedString::from))
        .collect::<Vec<_>>();
    Rc::new(VecModel::from(values)).into()
}

fn choice<T: Copy>(index: i32, labels: &[T]) -> Option<T> {
    usize::try_from(index)
        .ok()?
        .checked_sub(1)
        .and_then(|index| labels.get(index).copied())
}

fn artifact_row(entry: &Artifact, project: &Project) -> ArtifactRow {
    ArtifactRow {
        index: entry.id.0 as i32,
        name: entry.name().as_ref().into(),
        path: entry.path.to_string_lossy().as_ref().into(),
        logical_size: format_bytes(entry.measurement.logical_bytes).into(),
        context: format!(
            "{} • {} • {}",
            project.name(),
            entry.ecosystem.label(),
            entry.category.label()
        )
        .into(),
        status: format!("{} • {}", entry.review.label(), entry.confidence.label()).into(),
        partial: !entry.measured_completely,
    }
}

fn project_row(index: usize, project: &Project) -> ArtifactRow {
    let mut context = project
        .ecosystems
        .iter()
        .map(|value| value.label())
        .collect::<Vec<_>>();
    context.extend(project.tools.iter().copied());
    ArtifactRow {
        index: index as i32,
        name: project.name().as_ref().into(),
        path: project.root.to_string_lossy().as_ref().into(),
        logical_size: format_bytes(project.logical_bytes).into(),
        context: context.join(" • ").into(),
        status: if project.artifact_count == 1 {
            "1 artifact".into()
        } else {
            format!("{} artifacts", project.artifact_count).into()
        },
        partial: project.incomplete,
    }
}

fn details(index: usize, entry: &Artifact, project: &Project) -> ArtifactDetails {
    let files = entry.measurement.files;
    let directories = entry.measurement.directories;
    ArtifactDetails {
        open: true,
        index: index as i32,
        path: entry.path.to_string_lossy().as_ref().into(),
        association: format!("{} • {}", project.name(), entry.ecosystem.label()).into(),
        classification: format!(
            "{} • {} confidence",
            entry.review.label(),
            entry.confidence.label()
        )
        .into(),
        measurement: format!(
            "{} • {files} {} • {directories} {}{}",
            format_bytes(entry.measurement.logical_bytes),
            if files == 1 { "file" } else { "files" },
            if directories == 1 {
                "directory"
            } else {
                "directories"
            },
            if entry.measured_completely {
                ""
            } else {
                " • Partial measurement"
            }
        )
        .into(),
        evidence: format!(
            "{} — {}.",
            entry.evidence.project_marker, entry.evidence.artifact_marker
        )
        .into(),
        description: entry.description.into(),
        regeneration: entry.regeneration.into(),
        warning: entry.warning.into(),
    }
}
