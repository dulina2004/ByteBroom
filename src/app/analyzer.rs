use std::{cell::RefCell, collections::BTreeSet, path::PathBuf, rc::Rc, time::Duration};

use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::{
    analysis::{self, Catalog, Dataset, Item},
    storage::format_bytes,
};

use super::{AnalysisCard, AnalysisDetail, AnalysisEntry, AnalysisFilter, AnalysisTile, AppWindow};

const PAGE_SIZE: usize = 12;
const MINIMUMS: [u64; 4] = [100 << 20, 500 << 20, 1 << 30, 5 << 30];

#[derive(Default)]
pub(super) struct Analyzer {
    state: RefCell<View>,
    filter_timer: Timer,
    resize_timer: Timer,
}

struct View {
    scans: [Option<Catalog>; 2],
    global: Option<[Dataset; 6]>,
    snapshot: usize,
    mode: i32,
    route: Vec<usize>,
    directories: Vec<Item>,
    matches: Vec<usize>,
    filter: AnalysisFilter,
    facets: Vec<String>,
    selected: Option<usize>,
    page: usize,
    viewport: (f64, f64),
    chart_other: Option<u64>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scans: [None, None],
            global: None,
            snapshot: 0,
            mode: 0,
            route: Vec::new(),
            directories: Vec::new(),
            matches: Vec::new(),
            filter: AnalysisFilter::default(),
            facets: Vec::new(),
            selected: None,
            page: 0,
            viewport: (500.0, 300.0),
            chart_other: None,
        }
    }
}

impl Analyzer {
    pub(super) fn attach(window: &AppWindow) -> Rc<Self> {
        let analyzer = Rc::new(Self::default());
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_mode_changed(move |mode| {
            if let Some(window) = ui.upgrade() {
                view.filter_timer.stop();
                let mut state = view.state.borrow_mut();
                if state.mode != mode {
                    state.mode = mode;
                    window.set_analysis_mode(mode);
                    state.reset(&window);
                }
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_snapshot_changed(move |snapshot| {
            if let (Some(window), Ok(snapshot)) = (ui.upgrade(), usize::try_from(snapshot)) {
                view.filter_timer.stop();
                let mut state = view.state.borrow_mut();
                if state.scans.get(snapshot).is_some_and(Option::is_some) {
                    state.snapshot = snapshot;
                    state.route.clear();
                    state.reset(&window);
                }
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_filter_changed(move |filter| {
            let (view_inside, ui) = (Rc::downgrade(&view), ui.clone());
            view.filter_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(140),
                move || {
                    if let (Some(view), Some(window)) = (view_inside.upgrade(), ui.upgrade()) {
                        let mut state = view.state.borrow_mut();
                        state.filter = filter.clone();
                        state.page = 0;
                        state.apply(&window);
                    }
                },
            );
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_resize(move |width, height| {
            if width <= 0.0 || height <= 0.0 {
                return;
            }
            let (view_inside, ui) = (Rc::downgrade(&view), ui.clone());
            view.resize_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(90),
                move || {
                    if let (Some(view), Some(window)) = (view_inside.upgrade(), ui.upgrade()) {
                        let mut state = view.state.borrow_mut();
                        state.viewport = (f64::from(width), f64::from(height));
                        state.render_tiles(&window);
                    }
                },
            );
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_select(move |index| {
            if let (Some(window), Ok(index)) = (ui.upgrade(), usize::try_from(index)) {
                let mut state = view.state.borrow_mut();
                state.selected = Some(index);
                state.render_detail(&window);
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_enter(move || {
            if let Some(window) = ui.upgrade() {
                view.filter_timer.stop();
                let mut state = view.state.borrow_mut();
                if let Some(child) = state.selected.and_then(|id| state.items().get(id)?.child) {
                    state.route.push(child);
                    state.reset(&window);
                }
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_breadcrumb(move |depth| {
            if let (Some(window), Ok(depth)) = (ui.upgrade(), usize::try_from(depth)) {
                view.filter_timer.stop();
                let mut state = view.state.borrow_mut();
                state.route.truncate(depth);
                state.reset(&window);
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_page(move |delta| {
            if let Some(window) = ui.upgrade() {
                let mut state = view.state.borrow_mut();
                state.page = state.page.saturating_add_signed(delta.signum() as isize);
                state.render_rows(&window);
            }
        });
        let (view, ui) = (Rc::clone(&analyzer), window.as_weak());
        window.on_analysis_forget_previous(move || {
            if let Some(window) = ui.upgrade() {
                view.filter_timer.stop();
                let mut state = view.state.borrow_mut();
                state.scans[1] = None;
                if state.snapshot != 0 {
                    state.snapshot = 0;
                    state.route.clear();
                }
                state.reset(&window);
            }
        });
        analyzer.state.borrow_mut().reset(window);
        analyzer
    }

    pub(super) fn scan(&self, window: &AppWindow, result: Catalog) {
        self.filter_timer.stop();
        let mut state = self.state.borrow_mut();
        state.scans[1] = state.scans[0].take();
        state.scans[0] = Some(result);
        state.snapshot = 0;
        state.route.clear();
        state.reset(window);
    }

    #[cfg(windows)]
    pub(super) fn global(&self, window: &AppWindow, dataset: [Dataset; 6]) {
        let mut state = self.state.borrow_mut();
        state.global = Some(dataset);
        if state.mode >= 4 {
            self.filter_timer.stop();
            state.reset(window);
        }
    }

    pub(super) fn scan_path(&self) -> Option<PathBuf> {
        let state = self.state.borrow();
        let catalog = state.catalog()?;
        (state.mode == 0)
            .then(|| catalog.folder(&state.route).map(|(_, path)| path))
            .flatten()
    }
}

impl View {
    fn catalog(&self) -> Option<&Catalog> {
        self.scans[self.snapshot].as_ref()
    }

    fn dataset(&self) -> Option<&Dataset> {
        if self.mode >= 4 {
            return self.global.as_ref()?.get((self.mode - 4) as usize);
        }
        let catalog = self.catalog()?;
        match self.mode {
            1 => Some(&catalog.artifacts),
            2 => Some(&catalog.projects),
            3 => Some(&catalog.ecosystems),
            _ => None,
        }
    }

    fn items(&self) -> &[Item] {
        if self.mode == 0 {
            &self.directories
        } else {
            self.dataset().map_or(&[], |data| data.rows.as_slice())
        }
    }

    fn reset(&mut self, window: &AppWindow) {
        self.filter = AnalysisFilter::default();
        self.selected = None;
        self.page = 0;
        let folder = self
            .catalog()
            .and_then(|catalog| catalog.folder(&self.route));
        let (directories, path, root, available, limited) = if self.mode == 0 {
            folder
                .map(|(folder, path)| {
                    let root = self.catalog().map(|catalog| catalog.root.clone());
                    (folder.rows(&path), Some(path), root, true, !folder.expanded)
                })
                .unwrap_or_default()
        } else {
            (
                Vec::new(),
                self.catalog()
                    .filter(|_| self.mode < 4)
                    .map(|catalog| catalog.root.clone()),
                None,
                self.dataset().is_some(),
                false,
            )
        };
        self.directories = directories;
        let facets = self
            .items()
            .iter()
            .flat_map(|item| [item.category, item.status, item.risk])
            .filter(|value| !value.is_empty())
            .collect::<BTreeSet<_>>();
        self.facets = facets.into_iter().map(str::to_owned).collect();
        window.set_analysis_facets(model(
            std::iter::once("All categories and states".into())
                .chain(self.facets.iter().map(|name| name.as_str().into()))
                .collect(),
        ));
        window.set_analysis_available(available);
        window.set_analysis_limited(limited);
        window.set_analysis_snapshot(self.snapshot as i32);
        window.set_analysis_snapshots(model(
            ["Latest scan", "Previous scan"]
                .into_iter()
                .take(usize::from(self.scans[0].is_some()) + usize::from(self.scans[1].is_some()))
                .map(SharedString::from)
                .collect(),
        ));
        window.set_analysis_root(
            path.as_ref()
                .map(|path| path.to_string_lossy().as_ref().into())
                .unwrap_or_default(),
        );
        let breadcrumbs = root
            .as_ref()
            .map(|root| self.breadcrumbs(root))
            .unwrap_or_default();
        window.set_analysis_trail(model(breadcrumbs));
        self.render_overview(window, limited);
        self.render_comparison(window);
        self.apply(window);
    }

    fn render_comparison(&self, window: &AppWindow) {
        let mut rows = Vec::new();
        let mut note = String::new();
        if let [Some(current), Some(previous)] = &self.scans {
            match analysis::compare(previous, current) {
                Ok(comparison) => {
                    let [up, down, same, added, missing] = comparison.counts;
                    rows.extend([
                        format!(
                            "Previous {} · Current {} · Net {}",
                            format_bytes(comparison.previous),
                            format_bytes(comparison.current),
                            delta(i128::from(comparison.current) - i128::from(comparison.previous))
                        )
                        .into(),
                        format!("{up} increased · {down} decreased · {same} unchanged").into(),
                    ]);
                    if comparison.bounded {
                        note = "Directory changes compare entries retained in both scans. Root totals cover the whole measured scope.".into();
                    } else {
                        rows.push(
                            format!("{added} newly detected · {missing} no longer detected").into(),
                        );
                    }
                    rows.extend(comparison.changes.iter().take(PAGE_SIZE).map(|change| {
                        format!(
                            "{} · {} · {}",
                            change.name,
                            change.label,
                            delta(change.delta)
                        )
                        .into()
                    }));
                }
                Err(reason) => note = reason.into(),
            }
        }
        window.set_analysis_comparison(model(rows));
        window.set_analysis_comparison_note(note.into());
    }

    fn breadcrumbs(&self, root: &std::path::Path) -> Vec<SharedString> {
        let mut trail = vec![
            root.file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .as_ref()
                .into(),
        ];
        if let Some(catalog) = self.catalog() {
            let mut folder = &catalog.hierarchy;
            for &index in &self.route {
                folder = &folder.children[index];
                trail.push(folder.name.to_string_lossy().as_ref().into());
            }
        }
        trail
    }

    fn render_overview(&self, window: &AppWindow, limited: bool) {
        let mut cards = Vec::new();
        let mut notice = String::new();
        if self.mode == 0 {
            if let Some((folder, _)) = self
                .catalog()
                .and_then(|catalog| catalog.folder(&self.route))
            {
                cards.extend([
                    card(
                        "Scanned logical size",
                        format_bytes(folder.measurement.logical_bytes),
                    ),
                    card("Files", folder.measurement.files.to_string()),
                    card("Directories", folder.measurement.directories.to_string()),
                    card(
                        "Inaccessible entries",
                        folder.measurement.inaccessible.to_string(),
                    ),
                ]);
                if self.route.is_empty() || !folder.complete {
                    notice = self
                        .catalog()
                        .map(|catalog| catalog.notice.clone())
                        .unwrap_or_default();
                }
                if limited {
                    if !notice.is_empty() {
                        notice.push(' ');
                    }
                    notice.push_str("Child details were outside the retained depth. Analyze this folder to load them.");
                }
            }
        } else if let Some(dataset) = self.dataset() {
            let measured = analysis::sum(
                dataset
                    .rows
                    .iter()
                    .map(|row| row.bytes)
                    .chain([dataset.omitted]),
            );
            cards.extend([
                card(
                    if self.mode >= 4 {
                        "Measured logical size"
                    } else {
                        "Detected artifact size"
                    },
                    size(measured),
                ),
                card("Result count", dataset.count.to_string()),
            ]);
            if self.mode >= 4 {
                cards.push(card("Protected size", size(dataset.protected_bytes)));
            }
            if dataset.partial {
                notice.push_str("Partial results. Review individual measurement states.");
            }
            if dataset.count > dataset.rows.len() {
                if !notice.is_empty() {
                    notice.push(' ');
                }
                notice.push_str("This view searches the largest 512 entries. Browse the full retained inventory on Artifacts.");
            }
        }
        window.set_analysis_cards(model(cards));
        window.set_analysis_notice(notice.into());
    }

    fn apply(&mut self, window: &AppWindow) {
        let category = usize::try_from(self.filter.facet)
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| self.facets.get(index))
            .map_or("", String::as_str);
        let minimum = usize::try_from(self.filter.minimum)
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| MINIMUMS.get(index))
            .copied();
        self.matches = analysis::matching(
            self.items(),
            self.filter.query.as_str(),
            category,
            minimum,
            self.filter.sort,
        );
        if self.selected.is_some_and(|id| !self.matches.contains(&id)) {
            self.selected = None;
        }
        window.set_analysis_filter(self.filter.clone());
        let criteria = usize::from(!self.filter.query.trim().is_empty())
            + usize::from(!category.is_empty())
            + usize::from(minimum.is_some());
        let total = analysis::sum(self.matches.iter().map(|&index| self.items()[index].bytes));
        window.set_analysis_matches(
            format!(
                "{} results · {criteria} active filters · {} measured",
                self.matches.len(),
                size(total)
            )
            .into(),
        );
        self.render_rows(window);
        self.render_tiles(window);
        self.render_detail(window);
    }

    fn render_rows(&mut self, window: &AppWindow) {
        let pages = self.matches.len().div_ceil(PAGE_SIZE);
        self.page = self.page.min(pages.saturating_sub(1));
        let parent = (self.mode == 0)
            .then(|| self.catalog()?.folder(&self.route))
            .flatten()
            .map(|(folder, _)| folder.measurement.logical_bytes);
        let items = self.items();
        let rows = self
            .matches
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|&index| {
                let row = &items[index];
                let share = parent
                    .zip(row.bytes)
                    .filter(|(sum, _)| *sum != 0)
                    .map(|(sum, bytes)| {
                        format!(
                            "{:.1}% of measured folder",
                            bytes as f64 * 100.0 / sum as f64
                        )
                    })
                    .unwrap_or_default();
                AnalysisEntry {
                    id: index as i32,
                    name: row.name.as_str().into(),
                    size: size(row.bytes).into(),
                    subtitle: [
                        row.category,
                        row.group.as_str(),
                        row.status,
                        row.risk,
                        share.as_str(),
                    ]
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
                }
            })
            .collect();
        window.set_analysis_rows(model(rows));
        window.set_analysis_page_label(if pages == 0 {
            SharedString::default()
        } else {
            format!("{} / {pages}", self.page + 1).into()
        });
        window.set_analysis_previous_page(self.page > 0);
        window.set_analysis_next_page(self.page + 1 < pages);
    }

    fn render_tiles(&mut self, window: &AppWindow) {
        let items = self.items();
        let other = items.len();
        let weights = self
            .matches
            .iter()
            .filter_map(|&index| items[index].bytes.map(|bytes| (index, bytes)));
        let mut extra = None;
        let tiles = analysis::layout(weights, other, self.viewport.0, self.viewport.1)
            .into_iter()
            .map(|tile| {
                let row = items.get(tile.id);
                let bytes = Some(u64::try_from(tile.logical_bytes).unwrap_or(u64::MAX));
                if row.is_none() {
                    extra = bytes;
                }
                let name = row.map_or("Other visible results", |item| item.name.as_str());
                let tone = if row.is_some_and(|row| row.protected) {
                    5
                } else if row.is_none()
                    || row.is_some_and(|row| row.path.is_none() && self.mode == 0)
                {
                    4
                } else {
                    (tile.id % 4) as i32
                };
                AnalysisTile {
                    id: tile.id as i32,
                    name: name.into(),
                    size: size(bytes).into(),
                    tone,
                    left: tile.x as f32,
                    top: tile.y as f32,
                    wide: tile.width as f32,
                    tall: tile.height as f32,
                    label: tile.label,
                }
            })
            .collect();
        self.chart_other = extra;
        window.set_analysis_tiles(model(tiles));
    }

    fn render_detail(&self, window: &AppWindow) {
        let details = match self.selected {
            Some(index) if self.items().get(index).is_some() => {
                let item = &self.items()[index];
                AnalysisDetail { name: item.name.as_str().into(), size: size(item.bytes).into(),
                    path: item.path.as_ref().map(|path| path.to_string_lossy().as_ref().into()).unwrap_or_default(),
                    category: if item.group.is_empty() { item.category.into() } else { format!("{} · {}", item.category, item.group).into() },
                    description: item.description.as_str().into(), status: item.status.into(), risk: item.risk.into(),
                    enter: item.child.is_some() }
            }
            Some(index) if index == self.items().len() && self.chart_other.is_some() => AnalysisDetail {
                name: "Other visible results".into(), size: size(self.chart_other).into(),
                description: "Smaller measured rows grouped in the chart. Individual entries remain in the list.".into(),
                ..AnalysisDetail::default()
            },
            _ => AnalysisDetail::default(),
        };
        window.set_analysis_selected(self.selected.map_or(-1, |index| index as i32));
        window.set_analysis_details(details);
    }
}

fn card(label: &str, value: String) -> AnalysisCard {
    AnalysisCard {
        label: label.into(),
        value: value.into(),
    }
}
fn size(bytes: Option<u64>) -> String {
    bytes.map(format_bytes).unwrap_or_else(|| "Unknown".into())
}
fn delta(bytes: i128) -> String {
    let sign = if bytes > 0 {
        "+"
    } else if bytes < 0 {
        "−"
    } else {
        ""
    };
    format!(
        "{sign}{}",
        format_bytes(bytes.unsigned_abs().min(u128::from(u64::MAX)) as u64)
    )
}
fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}
