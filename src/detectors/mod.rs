mod model;
#[cfg(windows)]
mod rules;

pub(crate) use model::*;
#[cfg(windows)]
pub(crate) use rules::Facts;

#[cfg(windows)]
mod collection {
    use std::{
        ffi::OsStr,
        mem::size_of,
        path::{Path, PathBuf},
    };

    use super::{
        model::*,
        rules::{self, Facts, Folder, Relation},
    };

    const MAX_RECORDS: usize = 12_000;
    const PATH_BUDGET: usize = 16 * 1024 * 1024;

    pub(crate) struct Context {
        start: u64,
        folder: Folder,
        examined: bool,
        facts: Facts,
    }

    impl Context {
        pub(crate) fn root(path: &Path) -> Self {
            Self {
                start: 0,
                folder: path.file_name().map_or(Folder::Ordinary, |name| {
                    Folder::identify(name.as_encoded_bytes(), None)
                }),
                examined: true,
                facts: Facts::default(),
            }
        }

        pub(crate) fn examines_children(&self) -> bool {
            self.examined && self.folder.inspect_children()
        }

        pub(crate) fn wants_file_evidence(&self) -> bool {
            self.examined && self.folder.file_evidence()
        }

        pub(crate) fn file(&mut self, facts: Facts) {
            self.facts.merge(facts);
        }

        pub(crate) fn child(
            &mut self,
            name: Option<&OsStr>,
            parent: Option<&OsStr>,
            start: u64,
        ) -> Self {
            let examined = self.examines_children();
            let folder = name.map_or(Folder::Ordinary, |name| {
                let bytes = name.as_encoded_bytes();
                self.facts.note_directory(bytes);
                Folder::identify(bytes, parent.map(OsStr::as_encoded_bytes))
            });
            Self {
                start,
                folder,
                examined,
                facts: Facts::default(),
            }
        }
    }

    pub(crate) struct Collector {
        observations: Vec<Observation>,
        evidence_bytes: usize,
        truncated: bool,
        record_limit: usize,
    }

    impl Default for Collector {
        fn default() -> Self {
            Self {
                observations: Vec::new(),
                evidence_bytes: 0,
                truncated: false,
                record_limit: MAX_RECORDS,
            }
        }
    }

    #[derive(Default)]
    pub(crate) struct Observed {
        pub(crate) project: bool,
        pub(crate) candidate: bool,
    }

    struct Observation {
        path: PathBuf,
        facts: Facts,
        folder: Folder,
        ecosystem_mask: u8,
        start: u64,
        end: u64,
        measurement: Measurement,
        complete: bool,
    }

    impl Collector {
        #[cfg(test)]
        pub(crate) fn with_limit(record_limit: usize) -> Self {
            Self {
                record_limit,
                ..Self::default()
            }
        }

        pub(crate) fn observe(
            &mut self,
            context: Context,
            path: &Path,
            end: u64,
            measurement: Measurement,
            finished: bool,
        ) -> Observed {
            if !context.examined || context.folder == Folder::Protected {
                return Observed::default();
            }
            let ecosystem_mask = if context.examines_children() {
                Ecosystem::ALL
                    .into_iter()
                    .filter(|&ecosystem| context.facts.project_marker(ecosystem).is_some())
                    .fold(0, |mask, ecosystem| mask | ecosystem.bit())
            } else {
                0
            };
            let has_candidate = context.start != 0 && context.folder.candidate();
            if !has_candidate && ecosystem_mask == 0 {
                return Observed::default();
            }

            let overhead = path.as_os_str().len() + size_of::<Observation>();
            // Reserve part of the observation budget for project markers. In postorder
            // a project may close after thousands of its candidate directories.
            let (limit, budget) = if ecosystem_mask == 0 {
                (self.record_limit * 3 / 4, PATH_BUDGET * 3 / 4)
            } else {
                (self.record_limit, PATH_BUDGET)
            };
            if self.observations.len() >= limit || self.evidence_bytes + overhead > budget {
                self.truncated = true;
                return Observed::default();
            }
            self.evidence_bytes += overhead;
            self.observations.push(Observation {
                path: path.to_owned(),
                facts: context.facts,
                folder: context.folder,
                ecosystem_mask,
                start: context.start,
                end,
                measurement,
                complete: finished && measurement.inaccessible == 0,
            });
            Observed {
                project: ecosystem_mask != 0,
                candidate: has_candidate,
            }
        }

        // Ownership is resolved in discovery order (parents first) from bounded metadata.
        // Once a folder is classified as an artifact, its measured interval owns the
        // entire subtree and nested detections are discarded.
        pub(crate) fn finish(mut self, partial: bool) -> Results {
            self.observations
                .sort_unstable_by_key(|record| record.start);
            let mut result = Results {
                truncated: self.truncated,
                partial: partial || self.truncated,
                ..Results::default()
            };
            let mut project_mask = 0_u8;
            let mut scopes: Vec<Scope> = Vec::new();
            let mut skip_through = None;
            for record in self.observations {
                if skip_through.is_some_and(|end| record.start <= end) {
                    continue;
                }
                while scopes.last().is_some_and(|scope| scope.end < record.start) {
                    scopes.pop();
                }
                let finding =
                    rules::classify(record.folder, record.facts, |ecosystem, relation| {
                        owner(&scopes, &result.projects, &record.path, ecosystem, relation)
                    });
                if let Some((project, found)) = finding {
                    skip_through = Some(record.end);
                    let bytes = record.measurement.logical_bytes;
                    let owning = &mut result.projects[project.0];
                    owning.logical_bytes = owning.logical_bytes.saturating_add(bytes);
                    owning.artifact_count += 1;
                    owning.incomplete |= !record.complete;
                    result.logical_bytes = result.logical_bytes.saturating_add(bytes);
                    let (regeneration, warning) = effects(found.category, found.review);
                    result.artifacts.push(Artifact {
                        id: ArtifactId(result.artifacts.len()),
                        path: record.path,
                        project,
                        ecosystem: found.ecosystem,
                        category: found.category,
                        confidence: found.confidence,
                        review: found.review,
                        description: found.description,
                        regeneration,
                        warning,
                        evidence: found.evidence,
                        measurement: record.measurement,
                        measured_completely: record.complete,
                    });
                } else if record.ecosystem_mask != 0 {
                    project_mask |= record.ecosystem_mask;
                    scopes.push(Scope {
                        end: record.end,
                        facts: record.facts,
                        mask: record.ecosystem_mask,
                        project: ProjectId(result.projects.len()),
                    });
                    result.projects.push(Project {
                        root: record.path,
                        ecosystems: Ecosystem::ALL
                            .into_iter()
                            .filter(|ec| record.ecosystem_mask & ec.bit() != 0)
                            .collect(),
                        tools: record.facts.tools(),
                        logical_bytes: 0,
                        artifact_count: 0,
                        incomplete: false,
                    });
                }
            }
            result.ecosystem_count = project_mask.count_ones();
            result.artifacts.sort_unstable_by(|left, right| {
                right
                    .measurement
                    .logical_bytes
                    .cmp(&left.measurement.logical_bytes)
                    .then_with(|| left.path.cmp(&right.path))
            });
            result
        }
    }

    struct Scope {
        end: u64,
        facts: Facts,
        mask: u8,
        project: ProjectId,
    }

    fn owner(
        scopes: &[Scope],
        projects: &[Project],
        path: &Path,
        ecosystem: Ecosystem,
        relation: Relation,
    ) -> Option<(ProjectId, Facts)> {
        let ancestor = match relation {
            Relation::Parent => Some(path.parent()?),
            Relation::Grandparent => Some(path.parent()?.parent()?),
            Relation::Ancestor => None,
        };
        scopes
            .iter()
            .rev()
            .find(|scope| {
                scope.mask & ecosystem.bit() != 0
                    && ancestor.is_none_or(|path| projects[scope.project.0].root == path)
            })
            .map(|scope| (scope.project, scope.facts))
    }

    fn effects(category: Category, review: Review) -> (&'static str, &'static str) {
        match (category, review) {
            (Category::Environment, _) => (
                "Recreate with a compatible interpreter and environment specification.",
                "Exact dependencies, local packages, or interpreter settings may be difficult to reproduce.",
            ),
            (Category::Dependencies, _) => (
                "Reinstall with the original package manager and matching lockfile or toolchain.",
                "Regeneration can require network access and locally patched dependencies may be lost.",
            ),
            (Category::TestOutput, _) => (
                "Run the project’s coverage workflow again.",
                "Historical reports may be difficult to reproduce.",
            ),
            (Category::BuildOutput, Review::Manual) => (
                "Rebuild with the matching toolchain, configuration, and inputs.",
                "Review for exported or release deliverables that may be lost.",
            ),
            (Category::BuildOutput, _) => (
                "Rebuild with the matching toolchain and dependencies.",
                "Regeneration takes time and may require downloads.",
            ),
            (Category::Cache, _) => (
                "The corresponding tool recreates its cache when run again.",
                "The next run may take longer or need network access.",
            ),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn package_marker() -> Facts {
            let name: Vec<u8> = "package.json"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            Facts::from_windows_file(&name)
        }

        #[test]
        fn reserves_room_for_late_project_markers_and_bounds_retained_results() {
            let root = Path::new(r"C:\fixture");
            let mut parent = Context::root(root);
            parent.file(package_marker());
            let mut collector = Collector::with_limit(3);
            for index in 1..100 {
                let child = parent.child(
                    Some(OsStr::new("node_modules")),
                    Some(OsStr::new("fixture")),
                    index,
                );
                // Different snapshots are sufficient to exercise the retention limit.
                collector.observe(
                    child,
                    &root.join(format!("candidate-{index}")),
                    index,
                    Measurement {
                        logical_bytes: index,
                        ..Measurement::default()
                    },
                    true,
                );
            }
            assert_eq!(collector.observations.len(), 2);
            assert!(
                collector
                    .observe(parent, root, 99, Measurement::default(), true)
                    .project
            );
            let results = collector.finish(false);
            assert_eq!(results.projects.len(), 1);
            assert_eq!(results.artifacts.len(), 2);
            assert!(results.partial && results.truncated);
        }

        #[test]
        fn aggregates_u64_measurements_without_wrapping() {
            let root = Path::new(r"C:\fixture");
            let mut parent = Context::root(root);
            parent.file(package_marker());
            let mut collector = Collector::default();
            for (index, name, logical_bytes) in
                [(1, "node_modules", 5_u64 << 30), (2, ".next", u64::MAX - 1)]
            {
                let child =
                    parent.child(Some(OsStr::new(name)), Some(OsStr::new("fixture")), index);
                collector.observe(
                    child,
                    &root.join(name),
                    index,
                    Measurement {
                        logical_bytes,
                        ..Measurement::default()
                    },
                    true,
                );
            }
            collector.observe(parent, root, 2, Measurement::default(), true);
            let results = collector.finish(false);
            assert_eq!(results.artifacts[1].measurement.logical_bytes, 5_u64 << 30);
            assert_eq!(results.projects[0].logical_bytes, u64::MAX);
            assert_eq!(results.logical_bytes, u64::MAX);
        }
    }
}

#[cfg(windows)]
pub(crate) use collection::{Collector, Context};
