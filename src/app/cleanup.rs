use std::{cell::RefCell, rc::Rc};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{
    cleanup::{Candidate, Eligibility, Plan, Registry, Request, Session},
    storage::format_bytes,
};

use super::{AppWindow, CleanupItem, CleanupSummary};

const PAGE: usize = 12;

#[derive(Default)]
pub(super) struct CleanupView(RefCell<View>);

#[derive(Default)]
struct View {
    session: Session,
    query: String,
    facet: i32,
    page: usize,
    detail: Option<usize>,
}

impl CleanupView {
    pub(super) fn attach(window: &AppWindow) -> Rc<Self> {
        let view = Rc::new(Self::default());
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_filter(move |query, facet| {
            if let Some(window) = weak.upgrade() {
                let mut view = state.0.borrow_mut();
                view.query = query.trim().to_lowercase();
                view.facet = facet;
                view.page = 0;
                window.set_cleanup_query(query);
                window.set_cleanup_facet_index(facet);
                view.render_rows(&window);
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_page(move |offset| {
            if let Some(window) = weak.upgrade() {
                let mut view = state.0.borrow_mut();
                view.page = view.page.saturating_add_signed(offset.signum() as isize);
                view.render_rows(&window);
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_inspect(move |index| {
            if let Some(window) = weak.upgrade() {
                let mut view = state.0.borrow_mut();
                view.detail = usize::try_from(index).ok();
                view.render_detail(&window);
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_select(move |source, id, checked| {
            if let (Some(window), Ok(id)) = (weak.upgrade(), usize::try_from(id)) {
                state.0.borrow_mut().change(&window, |view| {
                    if source.as_str() == view.session.source_key() && !view.session.set_selected(id, checked) {
                        window.set_cleanup_notice(format!("This candidate is unavailable, excluded, or the {}-item selection limit has been reached.", crate::cleanup::SELECTION_LIMIT).into());
                    }
                });
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_exclude(move |source, id| {
            if let (Some(window), Ok(id)) = (weak.upgrade(), usize::try_from(id)) {
                state.0.borrow_mut().change(&window, |state| {
                    if source.as_str() == state.session.source_key() {
                        state.session.exclude(id);
                    }
                });
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_clear(move || {
            if let Some(window) = weak.upgrade() {
                state
                    .0
                    .borrow_mut()
                    .change(&window, |state| state.session.clear());
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_select_page(move || {
            if let Some(window) = weak.upgrade() {
                state.0.borrow_mut().change(&window, |state| {
                    let ids = state
                        .matches()
                        .iter()
                        .skip(state.page * PAGE)
                        .take(PAGE)
                        .map(|item| item.id)
                        .collect::<Vec<_>>();
                    state.session.select_visible(&ids);
                });
            }
        });
        let (state, weak) = (Rc::clone(&view), window.as_weak());
        window.on_cleanup_close_preview(move || {
            if let Some(window) = weak.upgrade() {
                state
                    .0
                    .borrow_mut()
                    .change(&window, |state| state.session.invalidate());
            }
        });
        view.0.borrow_mut().render(window);
        view
    }

    pub(super) fn replace(&self, window: &AppWindow, inventory: Registry) {
        let mut view = self.0.borrow_mut();
        view.session.replace(inventory);
        view.detail = None;
        view.page = 0;
        window.set_cleanup_notice("".into());
        view.render(window);
    }

    pub(super) fn invalidate(&self, window: &AppWindow) {
        let mut view = self.0.borrow_mut();
        view.session.invalidate();
        view.render_plan(window);
    }

    pub(super) fn request(&self, window: &AppWindow) -> Option<Request> {
        let mut view = self.0.borrow_mut();
        let request = view.session.request()?;
        window.set_cleanup_notice("".into());
        window.set_cleanup_progress("Starting metadata validation…".into());
        view.render_plan(window);
        Some(request)
    }

    pub(super) fn finish(&self, window: &AppWindow, result: Result<Plan, String>) {
        let mut view = self.0.borrow_mut();
        match result {
            Ok(plan) => {
                view.session.publish(plan);
            }
            Err(error) => {
                view.session.invalidate();
                window.set_cleanup_notice(error.into());
            }
        }
        view.render_plan(window);
    }
}

impl View {
    fn change(&mut self, window: &AppWindow, update: impl FnOnce(&mut Self)) {
        if window.get_cleanup_busy() {
            return;
        }
        window.set_cleanup_notice("".into());
        update(self);
        self.render(window);
    }

    fn matches(&self) -> Vec<&Candidate> {
        let mut rows = self
            .session
            .registry()
            .candidates
            .values()
            .map(std::sync::Arc::as_ref)
            .filter(|item| self.query.is_empty() || item.search.contains(&self.query))
            .filter(|item| match self.facet {
                1 => item.eligibility == Eligibility::Eligible,
                2 => item.eligibility == Eligibility::Review,
                3 => !item.eligibility.selectable(),
                4 => self.session.selected(item.id),
                5 => self.session.excluded(item.id),
                _ => true,
            })
            .collect::<Vec<_>>();
        rows.sort_unstable_by(|a, b| {
            b.measurement
                .logical_bytes
                .cmp(&a.measurement.logical_bytes)
                .then_with(|| a.path.cmp(&b.path))
                .then(a.id.cmp(&b.id))
        });
        rows
    }

    fn render(&mut self, window: &AppWindow) {
        let choices = &self.session.registry().candidates;
        let selected = self.session.selection().count();
        let eligible = choices
            .values()
            .filter(|item| item.eligibility.selectable())
            .count();
        let review = self
            .session
            .selection()
            .filter(|item| item.eligibility == Eligibility::Review)
            .count();
        window.set_cleanup_summary(CleanupSummary {
            selected: selected.to_string().into(),
            size: format_bytes(self.session.selected_bytes()).into(),
            eligible: eligible.to_string().into(),
            review: review.to_string().into(),
            unavailable: (choices.len() - eligible).to_string().into(),
        });
        window.set_cleanup_can_preview(selected > 0);
        window.set_cleanup_inventory_ready(!choices.is_empty());
        window.set_cleanup_coverage(if self.session.registry().omitted == 0 {
            "".into()
        } else {
            format!(
                "{} additional records remain in the Artifacts view.",
                self.session.registry().omitted
            )
            .into()
        });
        self.render_rows(window);
        self.render_detail(window);
        self.render_plan(window);
    }

    fn render_rows(&mut self, window: &AppWindow) {
        let rows = self.matches();
        let pages = rows.len().div_ceil(PAGE);
        let page = self.page.min(pages.saturating_sub(1));
        let visible = rows
            .iter()
            .skip(page * PAGE)
            .take(PAGE)
            .map(|item| project(item, &self.session))
            .collect();
        window.set_cleanup_matches(format!("{} matching candidates", rows.len()).into());
        self.page = page;
        window.set_cleanup_rows(model(visible));
        window.set_cleanup_page_label(if pages == 0 {
            "".into()
        } else {
            format!("{} / {pages}", page + 1).into()
        });
        window.set_cleanup_previous(page > 0);
        window.set_cleanup_next(page + 1 < pages);
    }

    fn render_detail(&self, window: &AppWindow) {
        let detail = self
            .detail
            .and_then(|id| self.session.registry().candidates.get(&id))
            .map(|item| project(item, &self.session))
            .unwrap_or_default();
        window.set_cleanup_detail(detail);
    }

    fn render_plan(&self, window: &AppWindow) {
        let Some(plan) = self.session.plan() else {
            window.set_cleanup_preview_ready(false);
            window.set_cleanup_plan_rows(ModelRc::default());
            return;
        };
        let bytes = plan
            .validated()
            .iter()
            .map(|item| item.measurement.logical_bytes)
            .fold(0, u64::saturating_add);
        let elapsed = plan.created().elapsed().unwrap_or_default().as_secs_f32();
        window.set_cleanup_plan_title(if plan.cancelled() {
            "Preview cancelled".into()
        } else {
            "Dry-run preview".into()
        });
        let drives = plan
            .validated()
            .iter()
            .filter_map(|item| {
                use std::path::{Component, Prefix};
                match item.path.components().next() {
                    Some(Component::Prefix(prefix)) => match prefix.kind() {
                        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                            Some(format!("{}:", char::from(letter.to_ascii_uppercase())))
                        }
                        _ => None,
                    },
                    _ => None,
                }
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        let volume = if drives.is_empty() {
            "No validated volumes"
        } else {
            drives.as_str()
        };
        window.set_cleanup_plan_summary(format!("{} validated · {} excluded · {} validated logical size · {volume} · policy {} · {elapsed:.1} s",
            plan.validated().len(), plan.rejected().len(), format_bytes(bytes), crate::cleanup::POLICY_VERSION).into());
        window.set_cleanup_plan_rows(model(
            plan.validated()
                .iter()
                .map(|item| {
                    let mut row = project(item, &self.session);
                    row.state = "Validated for preview".into();
                    row
                })
                .chain(plan.rejected().iter().map(|(item, reason)| {
                    let mut row = project(item, &self.session);
                    row.state = reason.label().into();
                    row
                }))
                .collect(),
        ));
        window.set_cleanup_preview_ready(true);
    }
}

fn project(item: &Candidate, state: &Session) -> CleanupItem {
    CleanupItem {
        source: state.source_key().into(),
        id: item.id as i32,
        name: item.name.as_str().into(),
        path: item.path.to_string_lossy().as_ref().into(),
        owner: item.project.to_string_lossy().as_ref().into(),
        category: format!("{} · {} · {}", item.ecosystem, item.category, item.risk).into(),
        size: format_bytes(item.measurement.logical_bytes).into(),
        evidence: format!("{} · {}", item.evidence, item.artifact_evidence).into(),
        description: item.description.into(),
        regeneration: item.regeneration.into(),
        warning: item.warning.into(),
        state: if state.excluded(item.id) {
            "Excluded this session".into()
        } else {
            item.eligibility.reason().into()
        },
        selectable: item.eligibility.selectable(),
        selected: state.selected(item.id),
        excluded: state.excluded(item.id),
        open: true,
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
}
