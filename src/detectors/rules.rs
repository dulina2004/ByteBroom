use super::model::{Category, Confidence, Ecosystem, Evidence, ProjectId, Review};

#[derive(Clone, Copy)]
enum Mark {
    Package,
    NodeLock,
    Next,
    Nuxt,
    Turbo,
    Parcel,
    Svelte,
    Angular,
    Vite,
    Bundler,
    Cargo,
    CargoLock,
    CargoOutput,
    Python,
    Pyvenv,
    Tox,
    Gradle,
    Settings,
    Maven,
    DotNet,
    Solution,
    DotNetOutput,
    Dart,
    Flutter,
    Coverage,
    NextOutput,
    Debug,
    Classes,
    GradleOutput,
    MavenOutput,
    FlutterAssets,
    AppOrWeb,
    DartTool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Facts(u64);

impl Facts {
    fn has(self, mark: Mark) -> bool {
        self.0 & (1 << mark as u64) != 0
    }
    fn add(&mut self, mark: Mark) {
        self.0 |= 1 << mark as u64;
    }
    pub(super) fn merge(&mut self, other: Self) {
        self.0 |= other.0;
    }

    // NT directory records provide UTF-16LE. Match short, ASCII marker names in place
    // and keep suffix recognition for arbitrary (including Unicode) .NET project names.
    pub(crate) fn from_windows_file(name: &[u8]) -> Self {
        let wide = name.as_chunks::<2>().0;
        let mut ascii = [0_u8; 32];
        let mark = if wide.len() <= ascii.len() && wide.iter().all(|unit| unit[1] == 0) {
            for (character, unit) in ascii.iter_mut().zip(wide) {
                *character = unit[0].to_ascii_lowercase();
            }
            match &ascii[..wide.len()] {
                b"package.json" => Some(Mark::Package),
                b"package-lock.json" | b"pnpm-lock.yaml" | b"yarn.lock" | b"bun.lock"
                | b"bun.lockb" => Some(Mark::NodeLock),
                b"turbo.json" => Some(Mark::Turbo),
                b".parcelrc" => Some(Mark::Parcel),
                b"angular.json" => Some(Mark::Angular),
                b"cargo.toml" => Some(Mark::Cargo),
                b"cargo.lock" => Some(Mark::CargoLock),
                b".rustc_info.json" => Some(Mark::CargoOutput),
                b"pyproject.toml" | b"requirements.txt" | b"pipfile" | b"poetry.lock"
                | b"setup.py" | b"environment.yml" => Some(Mark::Python),
                b"pyvenv.cfg" => Some(Mark::Pyvenv),
                b"tox.ini" | b".tox-info.json" => Some(Mark::Tox),
                b"build.gradle" | b"build.gradle.kts" => Some(Mark::Gradle),
                b"settings.gradle" | b"settings.gradle.kts" => Some(Mark::Settings),
                b"pom.xml" => Some(Mark::Maven),
                b"project.assets.json" => Some(Mark::DotNetOutput),
                b"pubspec.yaml" => Some(Mark::Dart),
                b".metadata" => Some(Mark::Flutter),
                b"lcov.info"
                | b"coverage-final.json"
                | b"coverage-summary.json"
                | b"clover.xml" => Some(Mark::Coverage),
                b"build_id" | b"routes-manifest.json" => Some(Mark::NextOutput),
                name => config_marker(name),
            }
        } else {
            None
        };
        let mark = mark.or_else(|| {
            const SUFFIXES: [(&[u8], Mark); 5] = [
                (b".csproj", Mark::DotNet),
                (b".fsproj", Mark::DotNet),
                (b".vbproj", Mark::DotNet),
                (b".sln", Mark::Solution),
                (b".slnx", Mark::Solution),
            ];
            SUFFIXES.into_iter().find_map(|(suffix, marker)| {
                let base = wide.len().checked_sub(suffix.len())?;
                (base != 0
                    && wide[base..]
                        .iter()
                        .zip(suffix)
                        .all(|(unit, byte)| unit[1] == 0 && unit[0].eq_ignore_ascii_case(byte)))
                .then_some(marker)
            })
        });
        let mut facts = Self::default();
        if let Some(mark) = mark {
            facts.add(mark);
        }
        facts
    }

    pub(super) fn note_directory(&mut self, name: &[u8]) {
        const MARKS: [(&[u8], Mark); 17] = [
            (b"debug", Mark::Debug),
            (b"release", Mark::Debug),
            (b"classes", Mark::Classes),
            (b"intermediates", Mark::GradleOutput),
            (b"outputs", Mark::GradleOutput),
            (b"kotlin", Mark::GradleOutput),
            (b"reports", Mark::GradleOutput),
            (b"maven-status", Mark::MavenOutput),
            (b"surefire-reports", Mark::MavenOutput),
            (b"maven-archiver", Mark::MavenOutput),
            (b"test-classes", Mark::MavenOutput),
            (b"generated-sources", Mark::MavenOutput),
            (b"flutter_assets", Mark::FlutterAssets),
            (b"app", Mark::AppOrWeb),
            (b"web", Mark::AppOrWeb),
            (b".dart_tool", Mark::DartTool),
            (b".fingerprint", Mark::CargoOutput),
        ];
        if let Some((_, marker)) = MARKS
            .into_iter()
            .find(|(text, _)| name.eq_ignore_ascii_case(text))
        {
            self.add(marker);
        }
    }

    pub(super) fn project_marker(self, ecosystem: Ecosystem) -> Option<&'static str> {
        let marks: &[(Mark, &str)] = match ecosystem {
            Ecosystem::JavaScript => &[
                (Mark::Package, "package.json"),
                (Mark::NodeLock, "JavaScript package lockfile"),
            ],
            Ecosystem::Cargo => &[(Mark::Cargo, "Cargo.toml"), (Mark::CargoLock, "Cargo.lock")],
            Ecosystem::Python => &[(
                Mark::Python,
                "Python project manifest, lockfile, or requirements file",
            )],
            Ecosystem::Gradle => &[
                (Mark::Gradle, "Gradle build script"),
                (Mark::Settings, "Gradle settings script"),
            ],
            Ecosystem::Maven => &[(Mark::Maven, "pom.xml")],
            Ecosystem::DotNet => &[
                (Mark::DotNet, ".NET project file"),
                (Mark::Solution, ".NET solution file"),
            ],
            Ecosystem::Dart => &[(Mark::Dart, "pubspec.yaml")],
        };
        marks
            .iter()
            .find_map(|&(marker, label)| self.has(marker).then_some(label))
    }

    pub(super) fn tools(self) -> Vec<&'static str> {
        [
            (Mark::Next, "Next.js"),
            (Mark::Nuxt, "Nuxt"),
            (Mark::Svelte, "Svelte"),
            (Mark::Angular, "Angular"),
            (Mark::Vite, "Vite"),
            (Mark::Turbo, "Turborepo"),
            (Mark::Parcel, "Parcel"),
            (Mark::Flutter, "Flutter project metadata"),
        ]
        .into_iter()
        .filter_map(|(mark, name)| self.has(mark).then_some(name))
        .collect()
    }
}

fn config_marker(name: &[u8]) -> Option<Mark> {
    let (base, extension) = name.split_at(name.iter().rposition(|&byte| byte == b'.')?);
    let extension = &extension[1..];
    if !matches!(extension, b"js" | b"ts" | b"cjs" | b"mjs" | b"cts" | b"mts") {
        return None;
    }
    match base {
        b"next.config" => Some(Mark::Next),
        b"nuxt.config" => Some(Mark::Nuxt),
        b"svelte.config" => Some(Mark::Svelte),
        b"vite.config" => Some(Mark::Vite),
        b"webpack.config" | b"rollup.config" => Some(Mark::Bundler),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Folder {
    #[default]
    Ordinary,
    Protected,
    NodeModules,
    Next,
    Nuxt,
    Turbo,
    Parcel,
    Svelte,
    Dist,
    Coverage,
    Target,
    Venv,
    Pycache,
    Pytest,
    Mypy,
    Ruff,
    Tox,
    Gradle,
    Build,
    Bin,
    Obj,
    DartTool,
    AngularCache,
    Vite,
}

impl Folder {
    pub(super) fn identify(name: &[u8], parent: Option<&[u8]>) -> Self {
        const FOLDERS: [(&[u8], Folder); 33] = [
            (b"node_modules", Folder::NodeModules),
            (b".next", Folder::Next),
            (b".nuxt", Folder::Nuxt),
            (b".turbo", Folder::Turbo),
            (b".parcel-cache", Folder::Parcel),
            (b".svelte-kit", Folder::Svelte),
            (b"dist", Folder::Dist),
            (b"coverage", Folder::Coverage),
            (b"target", Folder::Target),
            (b".venv", Folder::Venv),
            (b"venv", Folder::Venv),
            (b"__pycache__", Folder::Pycache),
            (b".pytest_cache", Folder::Pytest),
            (b".mypy_cache", Folder::Mypy),
            (b".ruff_cache", Folder::Ruff),
            (b".tox", Folder::Tox),
            (b".gradle", Folder::Gradle),
            (b"build", Folder::Build),
            (b"bin", Folder::Bin),
            (b"obj", Folder::Obj),
            (b".dart_tool", Folder::DartTool),
            (b".vite", Folder::Vite),
            (b".git", Folder::Protected),
            (b".cargo", Folder::Protected),
            (b".rustup", Folder::Protected),
            (b".m2", Folder::Protected),
            (b".npm", Folder::Protected),
            (b"npm-cache", Folder::Protected),
            (b".pnpm-store", Folder::Protected),
            (b".bun", Folder::Protected),
            (b".docker", Folder::Protected),
            (b"site-packages", Folder::Protected),
            (b".cache", Folder::Protected),
        ];
        if name.eq_ignore_ascii_case(b"cache")
            && parent.is_some_and(|p| p.eq_ignore_ascii_case(b".angular"))
        {
            return Self::AngularCache;
        }
        if name.eq_ignore_ascii_case(b"cache")
            && parent.is_some_and(|p| p.eq_ignore_ascii_case(b"pip"))
        {
            return Self::Protected;
        }
        FOLDERS
            .into_iter()
            .find(|(text, _)| name.eq_ignore_ascii_case(text))
            .map_or(Self::Ordinary, |(_, folder)| folder)
    }

    pub(super) fn candidate(self) -> bool {
        !matches!(self, Self::Ordinary | Self::Protected)
    }

    pub(super) fn inspect_children(self) -> bool {
        matches!(
            self,
            Self::Ordinary
                | Self::Dist
                | Self::Coverage
                | Self::Venv
                | Self::Target
                | Self::Build
                | Self::Bin
                | Self::Obj
        )
    }

    pub(super) fn file_evidence(self) -> bool {
        self.inspect_children() || matches!(self, Self::Next | Self::Tox)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Relation {
    Parent,
    Ancestor,
    Grandparent,
}

pub(super) struct Finding {
    pub(super) ecosystem: Ecosystem,
    pub(super) category: Category,
    pub(super) confidence: Confidence,
    pub(super) review: Review,
    pub(super) description: &'static str,
    pub(super) evidence: Evidence,
}

pub(super) fn classify(
    folder: Folder,
    facts: Facts,
    owner: impl Fn(Ecosystem, Relation) -> Option<(ProjectId, Facts)>,
) -> Option<(ProjectId, Finding)> {
    use Category::{BuildOutput, Cache, Dependencies, Environment, TestOutput};
    use Confidence::{Contextual, High};
    use Ecosystem::{Cargo, Dart, DotNet, Gradle, JavaScript, Maven, Python};
    use Relation::{Ancestor, Grandparent, Parent};
    let (ecosystem, relation, category, confidence, review, description, proof) = match folder {
        Folder::NodeModules => (
            JavaScript,
            Parent,
            Dependencies,
            High,
            Review::Regenerable,
            "Installed JavaScript dependencies.",
            "Conventional node_modules directory beside a project marker",
        ),
        Folder::Next => (
            JavaScript,
            Parent,
            BuildOutput,
            if facts.has(Mark::NextOutput) {
                High
            } else {
                Contextual
            },
            Review::Manual,
            "Next.js build output and cache.",
            if facts.has(Mark::NextOutput) {
                "Next.js BUILD_ID or routes manifest in .next"
            } else {
                "Conventional .next directory beside a JavaScript project marker"
            },
        ),
        Folder::Nuxt => (
            JavaScript,
            Parent,
            BuildOutput,
            Contextual,
            Review::Manual,
            "Nuxt generated files and build output.",
            "Conventional .nuxt directory beside a JavaScript project marker",
        ),
        Folder::Svelte => (
            JavaScript,
            Parent,
            BuildOutput,
            Contextual,
            Review::Regenerable,
            "SvelteKit generated files.",
            "Conventional .svelte-kit directory beside a JavaScript project marker",
        ),
        Folder::Turbo => (
            JavaScript,
            Parent,
            Cache,
            Contextual,
            Review::Cache,
            "Turborepo task cache.",
            "Conventional .turbo directory beside a JavaScript project marker",
        ),
        Folder::Parcel => (
            JavaScript,
            Parent,
            Cache,
            High,
            Review::Cache,
            "Parcel build cache.",
            "Conventional .parcel-cache directory beside a JavaScript project marker",
        ),
        Folder::AngularCache => (
            JavaScript,
            Grandparent,
            Cache,
            High,
            Review::Cache,
            "Angular CLI cache.",
            "The .angular/cache layout and angular.json",
        ),
        Folder::Vite => (
            JavaScript,
            Parent,
            Cache,
            Contextual,
            Review::Cache,
            "Vite project cache.",
            "The .vite directory and a Vite configuration filename",
        ),
        Folder::Dist => (
            JavaScript,
            Parent,
            BuildOutput,
            Contextual,
            Review::Manual,
            "Possible bundled or exported frontend output.",
            "The dist directory beside a Vite, Rollup, or webpack configuration",
        ),
        Folder::Coverage if facts.has(Mark::Coverage) => (
            JavaScript,
            Parent,
            TestOutput,
            High,
            Review::Manual,
            "Generated coverage reports.",
            "A recognized coverage report filename inside coverage",
        ),
        Folder::Venv if facts.has(Mark::Pyvenv) => (
            Python,
            Ancestor,
            Environment,
            High,
            Review::Regenerable,
            "Python virtual environment and installed packages.",
            "pyvenv.cfg in the virtual environment directory",
        ),
        Folder::Pycache => (
            Python,
            Ancestor,
            Cache,
            High,
            Review::Cache,
            "Compiled Python bytecode cache.",
            "Conventional __pycache__ within a Python project",
        ),
        Folder::Pytest => (
            Python,
            Ancestor,
            Cache,
            High,
            Review::Cache,
            "pytest test state and cache.",
            "Conventional .pytest_cache within a Python project",
        ),
        Folder::Mypy => (
            Python,
            Ancestor,
            Cache,
            High,
            Review::Cache,
            "mypy type-checking cache.",
            "Conventional .mypy_cache within a Python project",
        ),
        Folder::Ruff => (
            Python,
            Ancestor,
            Cache,
            High,
            Review::Cache,
            "Ruff lint and formatting cache.",
            "Conventional .ruff_cache within a Python project",
        ),
        Folder::Tox => (
            Python,
            Parent,
            Environment,
            Contextual,
            Review::Regenerable,
            "tox test environments.",
            "The .tox directory and tox configuration or metadata",
        ),
        Folder::Gradle => (
            Gradle,
            Parent,
            Cache,
            High,
            Review::Cache,
            "Project-local Gradle cache and state.",
            "The .gradle directory beside Gradle build or settings scripts",
        ),
        Folder::Bin => (
            DotNet,
            Parent,
            BuildOutput,
            if facts.has(Mark::Debug) {
                High
            } else {
                Contextual
            },
            Review::Manual,
            ".NET compiled output and possible deliverables.",
            "bin beside a .csproj, .fsproj, or .vbproj",
        ),
        Folder::Obj => (
            DotNet,
            Parent,
            BuildOutput,
            if facts.has(Mark::DotNetOutput) {
                High
            } else {
                Contextual
            },
            Review::Regenerable,
            ".NET build intermediates and restore metadata.",
            "obj beside a .csproj, .fsproj, or .vbproj",
        ),
        Folder::DartTool => (
            Dart,
            Parent,
            Cache,
            High,
            Review::Cache,
            "Dart and Flutter tooling state.",
            "Conventional .dart_tool directory beside pubspec.yaml",
        ),
        Folder::Target => {
            let cargo = owner(Cargo, Parent);
            let maven = owner(Maven, Parent);
            let cargo_layout = facts.has(Mark::Debug) || facts.has(Mark::CargoOutput);
            let maven_layout = facts.has(Mark::MavenOutput);
            if maven.is_some() && (cargo.is_none() || maven_layout && !cargo_layout) {
                (
                    Maven,
                    Parent,
                    BuildOutput,
                    if maven_layout { High } else { Contextual },
                    Review::Manual,
                    "Maven build output, test reports, and packaged deliverables.",
                    "The conventional target directory beside pom.xml",
                )
            } else if cargo.is_some_and(|(_, f)| f.has(Mark::Cargo) || cargo_layout)
                && (maven.is_none() || cargo_layout && !maven_layout)
            {
                (
                    Cargo,
                    Parent,
                    BuildOutput,
                    if cargo_layout { High } else { Contextual },
                    if cargo_layout {
                        Review::Regenerable
                    } else {
                        Review::Manual
                    },
                    "Cargo build output and incremental compiler state.",
                    "The conventional target directory beside Cargo project or workspace markers",
                )
            } else {
                return None;
            }
        }
        Folder::Build => {
            let dart = owner(Dart, Parent).is_some_and(|(_, f)| {
                facts.has(Mark::FlutterAssets)
                    || (f.has(Mark::Flutter) && f.has(Mark::DartTool) && facts.has(Mark::AppOrWeb))
            });
            let gradle = owner(Gradle, Ancestor).is_some()
                && (facts.has(Mark::GradleOutput)
                    || (facts.has(Mark::Classes) && owner(Gradle, Parent).is_some()));
            match (dart, gradle) {
                (true, false) => (
                    Dart,
                    Parent,
                    BuildOutput,
                    Contextual,
                    Review::Manual,
                    "Flutter build output and possible exports.",
                    "Flutter output layout in build beside pubspec.yaml",
                ),
                (false, true) => (
                    Gradle,
                    Ancestor,
                    BuildOutput,
                    Contextual,
                    Review::Manual,
                    "Gradle module build output and possible exports.",
                    "Generated output directories in build within a Gradle project",
                ),
                _ => return None,
            }
        }
        _ => return None,
    };
    let (project, project_facts) = owner(ecosystem, relation)?;
    let valid = match folder {
        Folder::AngularCache => project_facts.has(Mark::Angular),
        Folder::Vite => project_facts.has(Mark::Vite),
        Folder::Dist => project_facts.has(Mark::Vite) || project_facts.has(Mark::Bundler),
        Folder::Bin | Folder::Obj => project_facts.has(Mark::DotNet),
        Folder::Tox => project_facts.has(Mark::Tox) || facts.has(Mark::Tox),
        _ => true,
    };
    valid.then_some((
        project,
        Finding {
            ecosystem,
            category,
            confidence,
            review,
            description,
            evidence: Evidence {
                project_marker: project_facts.project_marker(ecosystem)?,
                artifact_marker: proof,
            },
        },
    ))
}
