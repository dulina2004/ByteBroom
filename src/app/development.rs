use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{
    global_caches::{self, CustomRoot, Inventory, Kind, Progress, Resource, Status, Tool},
    scanner::Cancellation,
    storage::format_bytes,
};

use super::{
    AppWindow, StorageDetails, StorageFilter, StorageGroup, StoragePhase, StorageRow,
    StorageSummary,
};

const PAGE_SIZE: usize = 10;

pub(super) enum Task {
    Discover(Option<Tool>, Vec<CustomRoot>),
    Measure(Inventory),
}

impl Task {
    pub(super) fn discover(&self) -> bool {
        matches!(self, Self::Discover(..))
    }

    pub(super) fn run(self, stop: &Cancellation, report: impl FnMut(Progress)) -> Inventory {
        match self {
            Self::Discover(tool, custom) => global_caches::discover(tool, &custom, stop, report),
            Self::Measure(inventory) => global_caches::measure(inventory, stop, report),
        }
    }
}

#[derive(Default)]
pub(super) struct StorageView(RefCell<State>);

#[derive(Default)]
struct State {
    generation: u64,
    inventory: Inventory,
    custom: Vec<CustomRoot>,
    scope_dirty: bool,
    filter: StorageFilter,
    matching: Vec<usize>,
    page: usize,
}

impl StorageView {
    pub(super) fn attach(window: &AppWindow) -> Rc<Self> {
        let view = Rc::new(Self::default());
        window.set_cache_providers(model(
            std::iter::once("All providers")
                .chain(Tool::ALL.iter().map(|tool| tool.name()))
                .map(SharedString::from)
                .collect(),
        ));

        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_add_custom(move |provider, path| {
            if let Some(window) = ui.upgrade() {
                state.add_custom(&window, provider, path.as_str());
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_clear_custom(move || {
            if let Some(window) = ui.upgrade() {
                let mut state = state.0.borrow_mut();
                state.custom.clear();
                state.scope_dirty = true;
                window.set_cache_custom_summary(
                    "Custom locations cleared. Discover again to update the preview.".into(),
                );
                window.set_cache_can_measure(false);
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_filter(move |mut filter| {
            if let Some(window) = ui.upgrade() {
                let mut state = state.0.borrow_mut();
                if state.filter.facet != filter.facet {
                    filter.choice = 0;
                }
                state.filter = filter;
                state.page = 0;
                state.apply_filter(&window);
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_turn(move |delta| {
            if let Some(window) = ui.upgrade() {
                let mut state = state.0.borrow_mut();
                state.page = state.page.saturating_add_signed(delta.signum() as isize);
                state.render_page(&window);
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_select(move |index, checked| {
            if let (Some(window), Ok(index)) = (ui.upgrade(), usize::try_from(index)) {
                let mut state = state.0.borrow_mut();
                if let Some(item) = state.inventory.resources.get_mut(index) {
                    item.selected = checked && item.can_select();
                    state.render_page(&window);
                    state.render_scope(&window);
                }
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_select_filtered(move |checked| {
            if let Some(window) = ui.upgrade() {
                let mut state = state.0.borrow_mut();
                let State {
                    inventory,
                    matching,
                    ..
                } = &mut *state;
                for &index in matching.iter() {
                    let item = &mut inventory.resources[index];
                    item.selected = checked && item.can_select();
                }
                state.render_page(&window);
                state.render_scope(&window);
            }
        });
        let (state, ui) = (Rc::clone(&view), window.as_weak());
        window.on_cache_inspect(move |index| {
            if let Some(window) = ui.upgrade() {
                let state = state.0.borrow();
                let item = usize::try_from(index)
                    .ok()
                    .and_then(|index| state.inventory.resources.get(index));
                window.set_cache_details(item.map(details).unwrap_or_default());
            }
        });
        view
    }

    fn add_custom(&self, window: &AppWindow, provider: i32, path: &str) {
        let Some(tool) = tool_at(provider) else {
            return;
        };
        let path = PathBuf::from(path.trim_matches('"'));
        let mut state = self.0.borrow_mut();
        let message = if global_caches::path_drive(&path).is_none() {
            "Enter an absolute local tool directory, such as D:\\ToolData\\cache.".into()
        } else if state
            .custom
            .iter()
            .any(|item| item.path == path && item.tool == tool)
        {
            "This custom location is already listed.".into()
        } else if state.custom.len() == global_caches::MAX_CUSTOM_ROOTS {
            format!(
                "Up to {} custom locations can be added per session.",
                global_caches::MAX_CUSTOM_ROOTS
            )
        } else {
            state.custom.push(CustomRoot { tool, path });
            state.scope_dirty = true;
            window.set_cache_can_measure(false);
            format!(
                "{} custom locations in this session. Discover again to update the preview.",
                state.custom.len()
            )
        };
        window.set_cache_custom_summary(message.into());
    }

    pub(super) fn discovery_task(&self, provider: i32) -> Task {
        Task::Discover(tool_at(provider), self.0.borrow().custom.clone())
    }

    pub(super) fn measurement_task(&self) -> Option<Task> {
        let state = self.0.borrow();
        (!state.scope_dirty
            && state
                .inventory
                .resources
                .iter()
                .any(|item| item.selected && item.can_select()))
        .then(|| Task::Measure(state.inventory.clone()))
    }

    pub(super) fn begin(&self, window: &AppWindow, discovery: bool) -> u64 {
        let mut state = self.0.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        if discovery {
            state.inventory = Inventory::default();
            state.scope_dirty = false;
            state.filter = StorageFilter::default();
            state.page = 0;
            state.matching.clear();
            window.set_cache_custom_summary(if state.custom.is_empty() {
                SharedString::default()
            } else {
                format!("{} session custom locations", state.custom.len()).into()
            });
            window.set_cache_scope("".into());
        }
        window.set_cache_details(StorageDetails::default());
        window.set_cache_notice("".into());
        window.set_cache_progress("Starting…".into());
        window.set_cache_phase(if discovery {
            StoragePhase::Discovering
        } else {
            StoragePhase::Measuring
        });
        state.generation
    }

    pub(super) fn progress(
        &self,
        window: &AppWindow,
        generation: u64,
        discovery: bool,
        progress: Progress,
    ) {
        if self.0.borrow().generation != generation {
            return;
        }
        let status = if discovery {
            format!(
                "{} tools · {} locations · {} s",
                progress.tools,
                progress.locations,
                progress.elapsed.as_secs()
            )
        } else {
            format!(
                "{} of {} locations processed · {} files examined · {} s",
                progress.processed,
                progress.locations,
                progress.files,
                progress.elapsed.as_secs()
            )
        };
        window.set_cache_progress(status.into());
    }

    pub(super) fn finish(
        &self,
        window: &AppWindow,
        generation: u64,
        discovery: bool,
        result: Result<Inventory, String>,
    ) {
        let mut state = self.0.borrow_mut();
        if state.generation != generation {
            return;
        }
        let (phase, notice) = match result {
            Ok(inventory) => {
                let phase = if inventory.cancelled {
                    StoragePhase::Cancelled
                } else if discovery {
                    StoragePhase::Ready
                } else {
                    StoragePhase::Completed
                };
                let notice = inventory_notice(&inventory, discovery);
                state.inventory = inventory;
                (phase, notice)
            }
            Err(message) => {
                if !discovery {
                    for item in &mut state.inventory.resources {
                        if item.can_select() {
                            item.status = Status::Ready;
                            item.measured = None;
                            item.skipped_links = 0;
                            item.avd = None;
                            item.note.clear();
                        }
                    }
                }
                (StoragePhase::Failed, message)
            }
        };
        state.page = 0;
        if state.filter.facet == 6 {
            state.filter.choice = 0;
        }
        state.render_summary(window);
        state.render_scope(window);
        state.apply_filter(window);
        window.set_cache_notice(notice.into());
        window.set_cache_phase(phase);
    }
}

impl State {
    fn apply_filter(&mut self, window: &AppWindow) {
        let options = filter_options(&self.inventory.resources, self.filter.facet);
        if self.filter.choice as usize > options.len() {
            self.filter.choice = 0;
        }
        let choice = usize::try_from(self.filter.choice)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| options.get(i));
        self.matching = matching(
            &self.inventory.resources,
            &self.filter,
            choice.map(String::as_str),
        );
        window.set_cache_filters(self.filter.clone());
        window.set_cache_choices(model(
            std::iter::once("All".into())
                .chain(options.into_iter().map(SharedString::from))
                .collect(),
        ));
        self.render_page(window);
    }

    fn render_page(&mut self, window: &AppWindow) {
        let pages = self.matching.len().div_ceil(PAGE_SIZE);
        self.page = self.page.min(pages.saturating_sub(1));
        let rows = self
            .matching
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|&index| row(&self.inventory.resources[index]))
            .collect();
        window.set_cache_rows(model(rows));
        window.set_cache_page_label(if pages == 0 {
            SharedString::default()
        } else {
            format!("{} / {pages}", self.page + 1).into()
        });
        window.set_cache_can_previous(self.page > 0);
        window.set_cache_can_next(self.page + 1 < pages);
    }

    fn render_scope(&self, window: &AppWindow) {
        let selected = self
            .inventory
            .resources
            .iter()
            .filter(|item| item.selected && item.can_select());
        let drives = selected
            .clone()
            .map(|item| item.drive)
            .collect::<BTreeSet<_>>();
        let count = selected.count();
        window.set_cache_can_measure(count > 0 && !self.scope_dirty);
        window.set_cache_scope(if self.inventory.resources.is_empty() { SharedString::default() } else {
            let drives = drives.into_iter().map(|drive| format!("{drive}:")).collect::<Vec<_>>().join(", ");
            if count == 0 { "No locations selected for measurement.".into() }
            else { format!("{count} selected on {drives}. Custom and environment locations start unchecked.").into() }
        });
    }

    fn render_summary(&self, window: &AppWindow) {
        let items = &self.inventory.resources;
        let tools = items
            .iter()
            .map(|item| item.tool)
            .collect::<BTreeSet<_>>()
            .len();
        let drives = items
            .iter()
            .map(|item| item.drive)
            .collect::<BTreeSet<_>>()
            .len();
        let review = items.iter().filter(|item| item.needs_review()).count();
        let protected = total(items.iter().filter(|item| item.kind == Kind::Protected));
        let logical = total(items.iter());
        let mut groups = BTreeMap::<_, u64>::new();
        for item in items {
            if let Some(measurement) = item.measured {
                let bytes = groups.entry(item.tool.group()).or_default();
                *bytes = bytes.saturating_add(measurement.logical_bytes);
            }
        }
        let max = groups.values().copied().max().unwrap_or(0).max(1);
        window.set_cache_groups(model(
            groups
                .into_iter()
                .map(|(group, bytes)| StorageGroup {
                    name: group.label().into(),
                    size: format_bytes(bytes).into(),
                    ratio: (bytes as f64 / max as f64) as f32,
                })
                .collect(),
        ));
        window.set_cache_summary(StorageSummary {
            tools: tools.to_string().into(),
            locations: items.len().to_string().into(),
            logical: size(logical),
            drives: drives.to_string().into(),
            review: review.to_string().into(),
            protected_size: size(protected),
        });
    }
}

fn inventory_notice(inventory: &Inventory, discovery: bool) -> String {
    let mut messages = Vec::new();
    if inventory.cancelled {
        messages.push(String::from("Scan cancelled. Results are partial."));
    }
    if inventory.truncated {
        messages.push(String::from(
            "Discovery reached a location or layout limit. Results are partial.",
        ));
    }
    if inventory
        .resources
        .iter()
        .any(|item| matches!(item.status, Status::Unavailable | Status::Partial))
    {
        messages.push(String::from(
            "Some locations could only be checked partially. Review their measurement status.",
        ));
    } else if !discovery && !inventory.cancelled {
        messages.push(String::from("Measurement finished."));
    }
    messages.extend(inventory.notices.iter().take(3).cloned());
    if inventory.notices.len() > 3 {
        messages.push(format!(
            "{} additional discovery notices.",
            inventory.notices.len() - 3
        ));
    }
    messages.join(" ")
}

fn matching(items: &[Resource], filter: &StorageFilter, choice: Option<&str>) -> Vec<usize> {
    let query = filter.query.trim().to_lowercase();
    let mut indices: Vec<_> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            choice.is_none_or(|value| filter_value(item, filter.facet) == value)
                && (query.is_empty()
                    || item.tool.name().to_lowercase().contains(&query)
                    || item.name.to_lowercase().contains(&query)
                    || item.path.to_string_lossy().to_lowercase().contains(&query))
        })
        .map(|(index, _)| index)
        .collect();
    indices.sort_by(|&a, &b| {
        let (left, right) = (&items[a], &items[b]);
        let by_name = || {
            left.tool
                .name()
                .cmp(right.tool.name())
                .then(left.name.cmp(&right.name))
                .then(left.path.cmp(&right.path))
        };
        if filter.sort == 2 {
            return by_name();
        }
        match (left.measured, right.measured) {
            (Some(x), Some(y)) => {
                let order = x.logical_bytes.cmp(&y.logical_bytes);
                if filter.sort == 1 {
                    order
                } else {
                    order.reverse()
                }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            _ => Ordering::Equal,
        }
        .then_with(by_name)
    });
    indices
}

fn filter_options(items: &[Resource], facet: i32) -> Vec<String> {
    items
        .iter()
        .map(|item| filter_value(item, facet))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn filter_value(item: &Resource, facet: i32) -> String {
    match facet {
        1 => item.tool.group().label(),
        2 => return format!("{}:", item.drive),
        3 => item.kind.label(),
        4 => item.risk(),
        5 => item.confidence.label(),
        6 => item.status.label(),
        _ => item.tool.name(),
    }
    .into()
}

fn row(item: &Resource) -> StorageRow {
    StorageRow {
        index: item.id as i32,
        title: item.name.as_str().into(),
        tool: item.tool.name().into(),
        path: item.path.to_string_lossy().as_ref().into(),
        size: size(item.measured.map(|value| value.logical_bytes)),
        category: item.kind.label().into(),
        drive: format!("{}:", item.drive).into(),
        classification: item.risk().into(),
        status: item.status.label().into(),
        selected: item.selected,
        selectable: item.can_select(),
    }
}

fn details(item: &Resource) -> StorageDetails {
    let (effects, restoration) = item.effects();
    let mut measurement = match item.measured {
        Some(value) => format!(
            "{} • {} logical • {} files • {} directories • {} inaccessible",
            item.status.label(),
            format_bytes(value.logical_bytes),
            value.files,
            value.directories,
            value.inaccessible
        ),
        None => format!("{} • Logical size unknown", item.status.label()),
    };
    if let Some(avd) = item.avd {
        measurement.push_str(&format!(
            " • {} disk images ({})",
            avd.disk_files,
            format_bytes(avd.disk_bytes)
        ));
        if let Some(bytes) = avd.snapshot_bytes {
            measurement.push_str(&format!(" • Snapshots {}", format_bytes(bytes)));
        }
    }
    if item.skipped_links != 0 {
        measurement.push_str(&format!(" • {} reparse points skipped", item.skipped_links));
    }
    StorageDetails {
        open: true,
        index: item.id as i32,
        path: item.path.to_string_lossy().as_ref().into(),
        source: item.source.label().into(),
        evidence: format!("{} confidence. {}", item.confidence.label(), item.evidence).into(),
        description: item.description.into(),
        effects: effects.into(),
        restoration: restoration.into(),
        measurement: measurement.into(),
        note: item.note.as_str().into(),
    }
}

fn total<'a>(items: impl Iterator<Item = &'a Resource>) -> Option<u64> {
    items
        .filter_map(|item| item.measured.map(|value| value.logical_bytes))
        .reduce(u64::saturating_add)
}

fn tool_at(provider: i32) -> Option<Tool> {
    usize::try_from(provider)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| Tool::ALL.get(index))
        .copied()
}

fn size(bytes: Option<u64>) -> SharedString {
    bytes
        .map(format_bytes)
        .unwrap_or_else(|| String::from("Unknown"))
        .into()
}

fn model<T: Clone + 'static>(data: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(data)).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Measurement;

    #[test]
    fn filters_cached_rows_and_preserves_large_and_unknown_sizes() {
        let fixture = tempfile::tempdir().unwrap();
        let roots = ["Large", "Empty", "Unknown"].map(|name| {
            let path = fixture.path().join(name);
            std::fs::create_dir(&path).unwrap();
            CustomRoot {
                tool: Tool::Maven,
                path,
            }
        });
        let mut inventory = global_caches::discover_fixture(Tool::Maven, &roots);
        let items = &mut inventory.resources;
        assert_eq!(items.len(), roots.len());
        items[0].measured = Some(Measurement {
            logical_bytes: 8 * 1024_u64.pow(4),
            ..Measurement::default()
        });
        items[1].measured = Some(Measurement::default());
        assert_eq!(size(total(items.iter())).as_str(), "8.0 TiB");
        assert_eq!(row(&items[2]).size.as_str(), "Unknown");
        assert_eq!(
            matching(items, &StorageFilter::default(), None),
            vec![0, 1, 2]
        );
        let search = StorageFilter {
            query: "uNkNoWn".into(),
            ..StorageFilter::default()
        };
        assert_eq!(matching(items, &search, Some("Maven")), vec![2]);
        assert!(matching(items, &search, Some("Cargo")).is_empty());
        let filter = StorageFilter {
            sort: 1,
            facet: 3,
            ..StorageFilter::default()
        };
        assert_eq!(
            matching(items, &filter, Some("Shared dependencies")),
            vec![1, 0, 2]
        );
    }
}
