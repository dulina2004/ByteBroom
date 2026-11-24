mod policy;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use policy::tracked;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use crate::{
    detectors::{Confidence, Measurement, Review},
    scanner::{Cancellation, ScanOutcome, ScanResult, ScopeIdentity},
};

use self::policy::Rule;

#[cfg(windows)]
pub(crate) const IDENTITY_LIMIT: usize = 1024;
const CANDIDATE_LIMIT: usize = 1024;
pub(crate) const SELECTION_LIMIT: usize = 64;
pub(crate) const POLICY_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Eligibility {
    Eligible,
    Review,
    Protected,
    Unsupported,
    Incomplete,
}

impl Eligibility {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Eligible => "Eligible for metadata preview",
            Self::Review => "Individual review required",
            Self::Protected => "Protected path or resource",
            Self::Unsupported => "Outside the preview allowlist",
            Self::Incomplete => "Requires a complete scan with verified identities",
        }
    }

    pub(crate) fn selectable(self) -> bool {
        matches!(self, Self::Eligible | Self::Review)
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Candidate {
    pub(crate) id: usize,
    pub(crate) path: PathBuf,
    pub(crate) project: PathBuf,
    pub(crate) name: String,
    pub(crate) ecosystem: &'static str,
    pub(crate) category: &'static str,
    pub(crate) risk: &'static str,
    pub(crate) eligibility: Eligibility,
    pub(crate) measurement: Measurement,
    pub(crate) evidence: &'static str,
    pub(crate) artifact_evidence: &'static str,
    pub(crate) description: &'static str,
    pub(crate) regeneration: &'static str,
    pub(crate) warning: &'static str,
    rule: Option<Rule>,
    identity: Option<ScopeIdentity>,
    project_identity: Option<ScopeIdentity>,
    pub(crate) search: String,
}

#[derive(Debug)]
struct Scope {
    path: PathBuf,
    identity: Option<ScopeIdentity>,
}

#[derive(Default)]
pub(crate) struct Registry {
    scope: Option<Arc<Scope>>,
    pub(crate) candidates: BTreeMap<usize, Arc<Candidate>>,
    pub(crate) omitted: usize,
}

impl Registry {
    pub(crate) fn from_scan(scan: &mut ScanResult) -> Self {
        let identities = std::mem::take(&mut scan.cleanup_identities);
        let complete = matches!(scan.outcome, ScanOutcome::Completed)
            && !scan.developer.partial
            && scan
                .identity
                .as_ref()
                .is_some_and(|identity| identity.1 != [0; 16]);
        let scope = Arc::new(Scope {
            path: scan.root.clone(),
            identity: scan.identity.clone(),
        });
        let mut candidates = BTreeMap::new();
        let mut omitted = 0;
        // Results retain scan IDs even where the preview inventory reaches its cap.
        for artifact in &scan.developer.artifacts {
            let Some(project) = scan.developer.projects.get(artifact.project.0) else {
                continue;
            };
            if candidates.len() == CANDIDATE_LIMIT {
                omitted += 1;
                continue;
            }
            let target = identities.get(&artifact.path);
            let owner = identities.get(&project.root);
            let rule = policy::rule(artifact);
            let eligibility = if policy::protected_path(&artifact.path)
                || target.is_some_and(|id| policy::protected_path(&id.2))
                || !policy::strict_child(&artifact.path, &project.root)
                || !artifact.path.starts_with(&scan.root)
                || !project.root.starts_with(&scan.root)
                || matches!(artifact.category, crate::detectors::Category::Environment)
            {
                Eligibility::Protected
            } else if rule.is_none() {
                Eligibility::Unsupported
            } else if !complete
                || !artifact.measured_completely
                || target.is_none()
                || owner.is_none()
            {
                Eligibility::Incomplete
            } else if artifact.review != Review::Cache || artifact.confidence != Confidence::High {
                Eligibility::Review
            } else {
                Eligibility::Eligible
            };
            let candidate = Candidate {
                id: artifact.id.0,
                name: artifact.name().into_owned(),
                path: artifact.path.clone(),
                project: project.root.clone(),
                ecosystem: artifact.ecosystem.label(),
                category: artifact.category.label(),
                risk: artifact.review.label(),
                eligibility,
                measurement: artifact.measurement,
                evidence: artifact.evidence.project_marker,
                artifact_evidence: artifact.evidence.artifact_marker,
                description: artifact.description,
                regeneration: artifact.regeneration,
                warning: artifact.warning,
                rule,
                identity: target.cloned(),
                project_identity: owner.cloned(),
                search: format!(
                    "{} {} {} {} {}",
                    artifact.path.to_string_lossy(),
                    project.root.to_string_lossy(),
                    artifact.ecosystem.label(),
                    artifact.category.label(),
                    artifact.review.label()
                )
                .to_lowercase(),
            };
            candidates.insert(candidate.id, Arc::new(candidate));
        }
        Self {
            candidates,
            scope: Some(scope),
            omitted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlanId {
    session: u64,
    scan: u64,
    revision: u64,
}

pub(crate) struct Request {
    id: PlanId,
    scope: Arc<Scope>,
    candidates: Vec<Arc<Candidate>>,
    excluded: Vec<Arc<Candidate>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Scope,
    Changed,
    Protected,
    Reparse,
    Inaccessible,
    Missing,
    Evidence,
    Overlap,
    Excluded,
    Limit,
    Cancelled,
    #[cfg(not(windows))]
    Unsupported,
}

impl Rejection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Scope => "Target falls outside the verified local scan or project",
            Self::Changed => "Identity or measured contents changed. Scan again",
            Self::Protected => "Protected or unexpected content was found",
            Self::Reparse => "Link, reparse point, or offline object encountered",
            Self::Inaccessible => "Windows could not validate all metadata",
            Self::Missing => "Target or required project directory is missing",
            Self::Evidence => "Required project or generated-output evidence is missing",
            Self::Overlap => "Overlaps another selected target",
            Self::Excluded => "Overlaps an excluded target",
            Self::Limit => "The validation depth or work limit was reached",
            Self::Cancelled => "Validation cancelled",
            #[cfg(not(windows))]
            Self::Unsupported => "Cleanup preview is unavailable on this platform",
        }
    }
}

pub(crate) struct Plan {
    id: PlanId,
    created: SystemTime,
    validated: Vec<Arc<Candidate>>,
    rejected: Vec<(Arc<Candidate>, Rejection)>,
    cancelled: bool,
}

impl Plan {
    pub(crate) fn created(&self) -> SystemTime {
        self.created
    }
    pub(crate) fn validated(&self) -> &[Arc<Candidate>] {
        &self.validated
    }
    pub(crate) fn rejected(&self) -> &[(Arc<Candidate>, Rejection)] {
        &self.rejected
    }
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled
    }
}

pub(crate) struct Session {
    id: u64,
    registry: Registry,
    selected: BTreeSet<usize>,
    excluded: BTreeSet<usize>,
    scan: u64,
    revision: u64,
    active: Option<PlanId>,
    plan: Option<Plan>,
}

impl Default for Session {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            registry: Registry::default(),
            selected: BTreeSet::new(),
            excluded: BTreeSet::new(),
            scan: 0,
            revision: 0,
            active: None,
            plan: None,
        }
    }
}

impl Session {
    pub(crate) fn replace(&mut self, registry: Registry) {
        self.scan = self.scan.wrapping_add(1);
        self.registry = registry;
        self.selected.clear();
        self.excluded.clear();
        self.invalidate();
    }

    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn source_key(&self) -> String {
        format!("{}-{}", self.id, self.scan)
    }
    pub(crate) fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }
    pub(crate) fn selected(&self, id: usize) -> bool {
        self.selected.contains(&id)
    }
    pub(crate) fn excluded(&self, id: usize) -> bool {
        self.excluded.contains(&id)
    }
    pub(crate) fn selection(&self) -> impl Iterator<Item = &Candidate> {
        self.selected
            .iter()
            .filter_map(|id| self.registry.candidates.get(id).map(Arc::as_ref))
    }

    pub(crate) fn selected_bytes(&self) -> u64 {
        let mut ordered: Vec<_> = self.selection().collect();
        ordered.sort_unstable_by_key(|item| (item.path.components().count(), item.id));
        let mut distinct = Vec::new();
        for item in ordered {
            if !distinct.iter().any(|&root| overlaps(item, root)) {
                distinct.push(item);
            }
        }
        distinct
            .into_iter()
            .map(|item| item.measurement.logical_bytes)
            .fold(0, u64::saturating_add)
    }

    pub(crate) fn set_selected(&mut self, id: usize, selected: bool) -> bool {
        if selected
            && (self.selected.len() == SELECTION_LIMIT
                || self.excluded(id)
                || !self
                    .registry
                    .candidates
                    .get(&id)
                    .is_some_and(|candidate| candidate.eligibility.selectable()))
        {
            return false;
        }
        if if selected {
            self.selected.insert(id)
        } else {
            self.selected.remove(&id)
        } {
            self.invalidate();
        }
        true
    }

    pub(crate) fn select_visible(&mut self, ids: &[usize]) {
        for &id in ids {
            if self
                .registry
                .candidates
                .get(&id)
                .is_some_and(|item| item.eligibility == Eligibility::Eligible)
            {
                self.set_selected(id, true);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.invalidate();
    }

    pub(crate) fn exclude(&mut self, id: usize) {
        if !self
            .registry
            .candidates
            .get(&id)
            .is_some_and(|item| item.eligibility.selectable())
        {
            return;
        }
        if !self.excluded.remove(&id) {
            self.excluded.insert(id);
            self.selected.remove(&id);
        }
        self.invalidate();
    }

    pub(crate) fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.active = None;
        self.plan = None;
    }

    pub(crate) fn request(&mut self) -> Option<Request> {
        if self.selected.is_empty() {
            return None;
        }
        let scope = self.registry.scope.clone()?;
        self.invalidate();
        let id = PlanId {
            session: self.id,
            scan: self.scan,
            revision: self.revision,
        };
        self.active = Some(id);
        Some(Request {
            id,
            scope,
            candidates: self
                .selected
                .iter()
                .map(|id| Arc::clone(&self.registry.candidates[id]))
                .collect(),
            excluded: self
                .excluded
                .iter()
                .map(|id| Arc::clone(&self.registry.candidates[id]))
                .collect(),
        })
    }

    pub(crate) fn publish(&mut self, plan: Plan) -> bool {
        if self.active != Some(plan.id) {
            return false;
        }
        self.active = None;
        self.plan = Some(plan);
        true
    }
}

pub(crate) fn preview(
    request: Request,
    stop: &Cancellation,
    progress: impl FnMut(usize, usize),
) -> Plan {
    build(request, stop, progress, validate)
}

fn build(
    mut request: Request,
    stop: &Cancellation,
    mut progress: impl FnMut(usize, usize),
    mut validate: impl FnMut(&Scope, &Candidate, &Cancellation) -> Result<(), Rejection>,
) -> Plan {
    let mut plan = Plan {
        id: request.id,
        created: SystemTime::now(),
        validated: Vec::new(),
        rejected: Vec::new(),
        cancelled: false,
    };
    request.candidates.sort_unstable_by(|a, b| {
        a.path
            .components()
            .count()
            .cmp(&b.path.components().count())
            .then(a.id.cmp(&b.id))
    });
    let total = request.candidates.len();
    for (index, item) in request.candidates.into_iter().enumerate() {
        progress(index, total);
        let result = if stop.is_cancelled() {
            Err(Rejection::Cancelled)
        } else if request
            .excluded
            .iter()
            .any(|excluded| overlaps(&item, excluded))
        {
            Err(Rejection::Excluded)
        } else if plan
            .validated
            .iter()
            .any(|validated| overlaps(&item, validated))
        {
            Err(Rejection::Overlap)
        } else {
            validate(&request.scope, &item, stop)
        };
        match if stop.is_cancelled() {
            Err(Rejection::Cancelled)
        } else {
            result
        } {
            Ok(()) => plan.validated.push(item),
            Err(error) => plan.rejected.push((item, error)),
        }
    }
    progress(total, total);
    plan.cancelled = stop.is_cancelled();
    plan
}

fn overlaps(a: &Candidate, b: &Candidate) -> bool {
    match (&a.identity, &b.identity) {
        (Some(a), Some(b)) if a.0 == b.0 => {
            a.1 == b.1 || a.2.starts_with(&b.2) || b.2.starts_with(&a.2)
        }
        _ => a.path.starts_with(&b.path) || b.path.starts_with(&a.path),
    }
}

fn validate(scope: &Scope, candidate: &Candidate, stop: &Cancellation) -> Result<(), Rejection> {
    if !candidate.eligibility.selectable() {
        return Err(Rejection::Protected);
    }
    #[cfg(windows)]
    return windows::validate(scope, candidate, stop);
    #[cfg(not(windows))]
    {
        let _ = (scope, stop);
        Err(Rejection::Unsupported)
    }
}

pub(crate) fn name(path: &Path) -> &str {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("")
}
