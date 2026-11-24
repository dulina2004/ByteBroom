use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::model::{Kind, Source, Tool};

pub(super) const VARIABLES: &[&str] = &[
    "npm_config_cache",
    "PNPM_HOME",
    "XDG_DATA_HOME",
    "YARN_CACHE_FOLDER",
    "YARN_GLOBAL_FOLDER",
    "BUN_INSTALL_CACHE_DIR",
    "GRADLE_USER_HOME",
    "PIP_CACHE_DIR",
    "UV_CACHE_DIR",
    "POETRY_CACHE_DIR",
    "POETRY_VIRTUALENVS_PATH",
    "CONDA_PKGS_DIRS",
    "CONDA_ENVS_PATH",
    "CARGO_HOME",
    "NUGET_PACKAGES",
    "NUGET_HTTP_CACHE_PATH",
    "NUGET_SCRATCH",
    "NUGET_PLUGINS_CACHE_PATH",
    "GOCACHE",
    "GOMODCACHE",
    "GOPATH",
    "PUB_CACHE",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "ANDROID_AVD_HOME",
    "ANDROID_EMULATOR_HOME",
    "ANDROID_USER_HOME",
    "PLAYWRIGHT_BROWSERS_PATH",
    "CYPRESS_CACHE_FOLDER",
    "PUPPETEER_CACHE_DIR",
    "HF_HOME",
    "HF_HUB_CACHE",
    "HF_DATASETS_CACHE",
    "TORCH_HOME",
    "XDG_CACHE_HOME",
];

#[derive(Default)]
pub(super) struct Environment {
    pub(super) home: Option<PathBuf>,
    pub(super) local: Option<PathBuf>,
    pub(super) roaming: Option<PathBuf>,
    pub(super) program_data: Option<PathBuf>,
    pub(super) temp: Option<PathBuf>,
    pub(super) variables: BTreeMap<&'static str, OsString>,
    pub(super) drives: Vec<char>,
}

#[derive(Clone, Debug)]
pub(crate) struct CustomRoot {
    pub(crate) tool: Tool,
    pub(crate) path: PathBuf,
}

#[derive(Clone)]
pub(super) struct Location {
    pub(super) path: PathBuf,
    pub(super) source: Source,
    // Validate the supplied base before joining known child paths.
    pub(super) base: PathBuf,
}

impl Location {
    fn new(path: PathBuf, source: Source) -> Self {
        Self {
            base: path.clone(),
            path,
            source,
        }
    }
    fn child(&self, relative: &str) -> Self {
        Self {
            path: self.path.join(relative),
            base: self.base.clone(),
            source: self.source,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Layout {
    Directory,
    PnpmStore,
    GradleCaches,
    AndroidApi,
    Versions,
    Avd,
    JetBrains,
}

#[derive(Clone, Copy)]
pub(super) struct Definition {
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    pub(super) description: &'static str,
}

pub(super) struct Plan {
    pub(super) tool: Tool,
    pub(super) location: Location,
    pub(super) definition: Definition,
    pub(super) layout: Layout,
}

struct Provider<'a> {
    tool: Tool,
    env: &'a Environment,
    custom: &'a [CustomRoot],
    plans: &'a mut Vec<Plan>,
}

impl Provider<'_> {
    fn configured(&self, keys: &[&'static str], default: Option<Location>) -> Option<Location> {
        setting(self.env, keys).or(default)
    }

    fn roots(&self, keys: &[&'static str], default: Option<Location>) -> Vec<Location> {
        let explicit = self
            .custom
            .iter()
            .filter(|root| root.tool == self.tool)
            .map(|root| Location::new(root.path.clone(), Source::Explicit))
            .collect::<Vec<_>>();
        if explicit.is_empty() {
            self.configured(keys, default).into_iter().collect()
        } else {
            explicit
        }
    }

    fn add(
        &mut self,
        location: Location,
        name: &'static str,
        kind: Kind,
        description: &'static str,
        layout: Layout,
    ) {
        self.plans.push(Plan {
            tool: self.tool,
            location,
            definition: Definition {
                name,
                kind,
                description,
            },
            layout,
        });
    }

    fn leaf(
        &mut self,
        location: Location,
        name: &'static str,
        kind: Kind,
        description: &'static str,
    ) {
        self.add(location, name, kind, description, Layout::Directory);
    }

    fn children(
        &mut self,
        location: &Location,
        details: &[(&str, &'static str, Kind, &'static str)],
    ) {
        for &(relative, name, kind, description) in details {
            self.leaf(location.child(relative), name, kind, description);
        }
    }
}

fn under(base: &Option<PathBuf>, relative: &str) -> Option<Location> {
    base.as_ref()
        .map(|base| Location::new(base.join(relative), Source::Default))
}

fn setting(env: &Environment, keys: &[&'static str]) -> Option<Location> {
    keys.iter().copied().find_map(|key| {
        env.variables
            .get(key)
            .map(|value| Location::new(PathBuf::from(value), Source::Environment(key)))
    })
}

pub(super) fn plans(env: &Environment, custom: &[CustomRoot], selected: Option<Tool>) -> Vec<Plan> {
    use Kind::{Cache, Dependencies, Installed, Protected, State};
    use Layout::*;
    let mut plans = Vec::new();
    for &tool in Tool::ALL {
        if selected.is_some_and(|choice| choice != tool) {
            continue;
        }
        let mut p = Provider {
            tool,
            env,
            custom,
            plans: &mut plans,
        };
        match tool {
            Tool::Npm => {
                for root in p.roots(&["npm_config_cache"], under(&env.local, "npm-cache")) {
                    p.leaf(
                        root.child("_cacache"),
                        "Download cache",
                        Cache,
                        "npm content-addressed package downloads and HTTP responses.",
                    );
                }
            }
            Tool::Pnpm => {
                let default = setting(env, &["PNPM_HOME"])
                    .map(|root| root.child("store"))
                    .or_else(|| {
                        setting(env, &["XDG_DATA_HOME"]).map(|root| root.child("pnpm/store"))
                    })
                    .or_else(|| under(&env.local, "pnpm/store"));
                let mut roots = p.roots(&[], default);
                if !custom.iter().any(|root| root.tool == Tool::Pnpm) {
                    roots.extend(env.drives.iter().map(|drive| {
                        Location::new(
                            PathBuf::from(format!("{drive}:\\.pnpm-store")),
                            Source::Default,
                        )
                    }));
                }
                for root in roots {
                    p.add(root, "Package store", Dependencies, "A content-addressed store shared by pnpm projects. Hard-linked files can appear in both this store and project totals.", PnpmStore);
                }
            }
            Tool::Yarn => {
                for root in p.roots(&["YARN_CACHE_FOLDER"], under(&env.local, "Yarn/Cache")) {
                    let name = if root.source == Source::Default {
                        "Classic global cache"
                    } else {
                        "Configured cache folder"
                    };
                    p.leaf(root, name, Dependencies, "Yarn package archives. A configured folder may contain offline or version-controlled archives.");
                }
                if let Some(root) =
                    p.configured(&["YARN_GLOBAL_FOLDER"], under(&env.home, ".yarn/berry"))
                {
                    p.leaf(
                        root.child("cache"),
                        "Modern global archives",
                        Dependencies,
                        "Modern Yarn's global package archives used by development projects.",
                    );
                }
            }
            Tool::Bun => {
                for root in p.roots(
                    &["BUN_INSTALL_CACHE_DIR"],
                    under(&env.home, ".bun/install/cache"),
                ) {
                    p.leaf(root, "Package download cache", Cache, "Downloaded packages used by Bun installs. Hard links can make logical and reclaimable sizes differ.");
                }
            }
            Tool::Gradle => {
                for root in p.roots(&["GRADLE_USER_HOME"], under(&env.home, ".gradle")) {
                    p.add(root.child("caches"), "Gradle caches", Cache, "Known dependency, task-output, and transform subdivisions of the Gradle caches directory.", GradleCaches);
                    p.children(
                        &root,
                        &[
                            (
                                "wrapper/dists",
                                "Wrapper distributions",
                                Installed,
                                "Downloaded Gradle versions selected by Gradle wrappers.",
                            ),
                            (
                                "daemon",
                                "Daemon logs and registry",
                                State,
                                "Daemon logs and Gradle daemon coordination state.",
                            ),
                            (
                                "jdks",
                                "Downloaded Java toolchains",
                                Installed,
                                "JDKs provisioned for builds that request a toolchain.",
                            ),
                        ],
                    );
                }
            }
            Tool::Maven => {
                for root in p.roots(&[], under(&env.home, ".m2/repository")) {
                    p.leaf(root, "Local artifact repository", Dependencies, "Downloaded and locally installed Maven artifacts. Local builds may be absent from every remote repository.");
                }
            }
            Tool::Pip => {
                for root in p.roots(&["PIP_CACHE_DIR"], under(&env.local, "pip/Cache")) {
                    p.children(
                        &root,
                        &[
                            (
                                "http-v2",
                                "HTTP cache",
                                Cache,
                                "Cached HTTP responses from package repositories.",
                            ),
                            (
                                "http",
                                "Legacy HTTP cache",
                                Cache,
                                "Cached responses using pip's older cache format.",
                            ),
                            (
                                "wheels",
                                "Wheel cache",
                                Cache,
                                "Built Python wheels cached by pip.",
                            ),
                        ],
                    );
                }
            }
            Tool::Uv => {
                for root in p.roots(&["UV_CACHE_DIR"], under(&env.local, "uv/cache")) {
                    p.leaf(root, "Package cache", Cache, "uv's shared package and wheel cache. Project environments can share file contents through links.");
                }
            }
            Tool::Poetry => {
                for root in p.roots(&["POETRY_CACHE_DIR"], under(&env.local, "pypoetry/Cache")) {
                    p.children(
                        &root,
                        &[
                            (
                                "artifacts",
                                "Downloaded artifacts",
                                Cache,
                                "Downloaded Python distribution artifacts.",
                            ),
                            (
                                "cache/repositories",
                                "Repository cache",
                                Cache,
                                "Poetry's cached package repository responses.",
                            ),
                        ],
                    );
                    if !env.variables.contains_key("POETRY_VIRTUALENVS_PATH") {
                        p.leaf(root.child("virtualenvs"), "Virtual environments", State, "Installed Poetry environments that may contain local packages and state.");
                    }
                }
                if let Some(root) = setting(env, &["POETRY_VIRTUALENVS_PATH"]) {
                    p.leaf(
                        root,
                        "Virtual environments",
                        State,
                        "Installed Poetry environments that may contain local packages and state.",
                    );
                }
            }
            Tool::Conda => {
                let custom_paths = p.roots(&[], None);
                let packages = if custom_paths.is_empty() {
                    list_setting(
                        env,
                        "CONDA_PKGS_DIRS",
                        Some(','),
                        under(&env.home, ".conda/pkgs"),
                    )
                } else {
                    custom_paths
                };
                for root in packages {
                    p.leaf(root, "Shared packages", Dependencies, "Cached Conda archives and extracted packages. Conda environments may hard-link these files.");
                }
                for root in list_setting(
                    env,
                    "CONDA_ENVS_PATH",
                    None,
                    under(&env.home, ".conda/envs"),
                ) {
                    p.leaf(root, "Conda environments", State, "Installed environments, including local packages and active development state.");
                }
            }
            Tool::Cargo => {
                for root in p.roots(&["CARGO_HOME"], under(&env.home, ".cargo")) {
                    p.children(&root, &[
                        ("registry/cache", "Registry archives", Cache, "Downloaded compressed crate archives."),
                        ("registry/index", "Registry index", Cache, "Registry package index data."),
                        ("registry/src", "Extracted crate sources", Dependencies, "Extracted registry dependencies shared by Cargo projects."),
                        ("git/db", "Git dependency databases", Dependencies, "Cached Git dependency repositories."),
                        ("git/checkouts", "Git dependency checkouts", Dependencies, "Materialized Cargo Git dependencies, which may contain local changes."),
                    ]);
                }
            }
            Tool::Nuget => {
                for root in p.roots(&["NUGET_PACKAGES"], under(&env.home, ".nuget/packages")) {
                    p.leaf(
                        root,
                        "Global packages",
                        Dependencies,
                        "Unpacked NuGet packages shared across projects and offline restores.",
                    );
                }
                for (key, default, name, detail) in [
                    (
                        "NUGET_HTTP_CACHE_PATH",
                        under(&env.local, "NuGet/v3-cache"),
                        "HTTP cache",
                        "Responses cached by NuGet package sources.",
                    ),
                    (
                        "NUGET_SCRATCH",
                        under(&env.temp, "NuGetScratch"),
                        "Temporary cache",
                        "Temporary NuGet storage, which can be in use by active restores.",
                    ),
                    (
                        "NUGET_PLUGINS_CACHE_PATH",
                        under(&env.local, "NuGet/plugins-cache"),
                        "Plugin cache",
                        "NuGet plugin results and capability cache.",
                    ),
                ] {
                    if let Some(root) = p.configured(&[key], default) {
                        p.leaf(
                            root,
                            name,
                            if key == "NUGET_SCRATCH" { State } else { Cache },
                            detail,
                        );
                    }
                }
            }
            Tool::Go => {
                if env
                    .variables
                    .get("GOCACHE")
                    .is_none_or(|value| value != "off")
                    && let Some(root) = p.configured(&["GOCACHE"], under(&env.local, "go-build"))
                {
                    p.leaf(
                        root,
                        "Compiler build cache",
                        Cache,
                        "Compiled Go build results used across packages.",
                    );
                }
                let default = env
                    .variables
                    .get("GOPATH")
                    .map(|value| {
                        let base = std::env::split_paths(value).next().unwrap_or_default();
                        Location::new(base, Source::Environment("GOPATH")).child("pkg/mod")
                    })
                    .or_else(|| under(&env.home, "go/pkg/mod"));
                for root in p.roots(&["GOMODCACHE"], default) {
                    p.leaf(
                        root,
                        "Module cache",
                        Dependencies,
                        "Downloaded and extracted Go modules shared across projects.",
                    );
                }
            }
            Tool::Pub => {
                for root in p.roots(&["PUB_CACHE"], under(&env.local, "Pub/Cache")) {
                    p.children(
                        &root,
                        &[
                            (
                                "hosted",
                                "Hosted dependencies",
                                Dependencies,
                                "Dart and Flutter packages downloaded from package registries.",
                            ),
                            (
                                "git",
                                "Git dependencies",
                                Dependencies,
                                "Cached Git packages and checkouts used by Pub.",
                            ),
                        ],
                    );
                }
            }
            Tool::AndroidSdk => {
                for root in p.roots(
                    &["ANDROID_HOME", "ANDROID_SDK_ROOT"],
                    under(&env.local, "Android/Sdk"),
                ) {
                    for (subdir, name, detail, layout) in [
                        (
                            "platforms",
                            "SDK platform",
                            "Installed SDK components for an Android API level.",
                            AndroidApi,
                        ),
                        (
                            "build-tools",
                            "Build tools",
                            "Installed Android compiler, packaging and build tools.",
                            Versions,
                        ),
                        (
                            "system-images",
                            "System images",
                            "Installed emulator system images grouped by Android API level.",
                            AndroidApi,
                        ),
                        (
                            "ndk",
                            "NDK",
                            "Installed Android native development toolchain.",
                            Versions,
                        ),
                        (
                            "cmake",
                            "CMake",
                            "Installed Android CMake toolchain.",
                            Versions,
                        ),
                    ] {
                        p.add(root.child(subdir), name, Installed, detail, layout);
                    }
                    p.children(
                        &root,
                        &[
                            (
                                "platform-tools",
                                "Platform tools",
                                Installed,
                                "Android platform tools such as adb and fastboot.",
                            ),
                            (
                                "emulator",
                                "Emulator binaries",
                                Installed,
                                "The installed Android emulator executable and libraries.",
                            ),
                        ],
                    );
                }
            }
            Tool::AndroidAvd => {
                let default = setting(env, &["ANDROID_EMULATOR_HOME", "ANDROID_USER_HOME"])
                    .map(|location| location.child("avd"))
                    .or_else(|| under(&env.home, ".android/avd"));
                for root in p.roots(&["ANDROID_AVD_HOME"], default) {
                    p.add(root, "Virtual device", Protected, "Android virtual device state, including virtual disks, snapshots, installed apps, and test data.", Avd);
                }
            }
            Tool::JetBrains => {
                let explicit = p.roots(&[], None);
                let roots = if explicit.is_empty() {
                    [under(&env.local, "JetBrains"), under(&env.local, "Google")]
                        .into_iter()
                        .flatten()
                        .collect()
                } else {
                    explicit
                };
                for root in roots {
                    p.add(
                        root,
                        "IDE storage",
                        State,
                        "Recognized version-specific IDE data.",
                        JetBrains,
                    );
                }
            }
            Tool::VsCode => {
                let explicit = p.roots(&[], None);
                let roots = if explicit.is_empty() {
                    for (suffix, label) in [
                        (".vscode/extensions", "Extensions"),
                        (".vscode-insiders/extensions", "Insiders extensions"),
                    ] {
                        if let Some(root) = under(&env.home, suffix) {
                            p.leaf(root, label, Installed, "Installed editor extensions. Some are locally installed or pinned to a specific version.");
                        }
                    }
                    [
                        under(&env.roaming, "Code"),
                        under(&env.roaming, "Code - Insiders"),
                    ]
                    .into_iter()
                    .flatten()
                    .collect()
                } else {
                    explicit
                };
                for root in roots {
                    p.children(
                        &root,
                        &[
                            (
                                "Cache",
                                "Application cache",
                                Cache,
                                "Editor application cache, scoped to the known user-data layout.",
                            ),
                            (
                                "CachedData",
                                "Runtime cache",
                                Cache,
                                "Cached editor runtime data.",
                            ),
                            (
                                "CachedExtensionVSIXs",
                                "Extension downloads",
                                Cache,
                                "Downloaded extension installation archives.",
                            ),
                            (
                                "logs",
                                "Editor logs",
                                State,
                                "Editor and extension-host diagnostic logs.",
                            ),
                            (
                                "User/globalStorage",
                                "Global extension data",
                                State,
                                "Persistent editor and extension state shared across workspaces.",
                            ),
                            (
                                "User/workspaceStorage",
                                "Workspace storage",
                                State,
                                "Per-workspace editor and extension state.",
                            ),
                            (
                                "Backups",
                                "Unsaved file backups",
                                Protected,
                                "Recovery data for potentially unsaved work.",
                            ),
                            (
                                "User/History",
                                "Local history",
                                Protected,
                                "Editor history that can contain work absent from version control.",
                            ),
                        ],
                    );
                }
            }
            Tool::Playwright => {
                let roots = p.roots(
                    &["PLAYWRIGHT_BROWSERS_PATH"],
                    under(&env.local, "ms-playwright"),
                );
                for root in roots.into_iter().filter(|root| {
                    !(root.source == Source::Environment("PLAYWRIGHT_BROWSERS_PATH")
                        && root.path == Path::new("0"))
                }) {
                    p.leaf(
                        root,
                        "Automation browsers",
                        Installed,
                        "Browser builds downloaded for Playwright testing.",
                    );
                }
            }
            Tool::Cypress => {
                for root in p.roots(
                    &["CYPRESS_CACHE_FOLDER"],
                    under(&env.local, "Cypress/Cache"),
                ) {
                    p.leaf(
                        root,
                        "Cypress binaries",
                        Installed,
                        "Versioned Cypress test-runner downloads.",
                    );
                }
            }
            Tool::Puppeteer => {
                for root in p.roots(
                    &["PUPPETEER_CACHE_DIR"],
                    under(&env.home, ".cache/puppeteer"),
                ) {
                    p.children(
                        &root,
                        &[
                            (
                                "chrome",
                                "Chrome for testing",
                                Installed,
                                "Puppeteer-managed Chrome automation downloads.",
                            ),
                            (
                                "chrome-headless-shell",
                                "Headless Chrome",
                                Installed,
                                "Puppeteer-managed Chrome headless shell downloads.",
                            ),
                            (
                                "firefox",
                                "Firefox for testing",
                                Installed,
                                "Puppeteer-managed Firefox automation downloads.",
                            ),
                        ],
                    );
                }
            }
            Tool::HuggingFace => {
                let base = cache_home(env, "HF_HOME", "huggingface");
                for root in p.roots(
                    &["HF_HUB_CACHE"],
                    base.as_ref().map(|base| base.child("hub")),
                ) {
                    p.leaf(root, "Hub models and repositories", State, "Cached model weights, datasets, and repository snapshots. Private or removed revisions may be unavailable.");
                }
                if let Some(root) = p.configured(
                    &["HF_DATASETS_CACHE"],
                    base.map(|base| base.child("datasets")),
                ) {
                    p.leaf(root, "Dataset cache", State, "Processed datasets and dataset download data that may be expensive or impossible to regenerate.");
                }
            }
            Tool::Pytorch => {
                for root in p.roots(
                    &[],
                    cache_home(env, "TORCH_HOME", "torch").map(|base| base.child("hub")),
                ) {
                    p.leaf(root, "Hub downloads and checkpoints", State, "Hub repositories and weights. Checkpoints and locally modified models may be difficult to recover.");
                }
            }
        }
    }
    plans
}

fn cache_home(
    env: &Environment,
    setting_name: &'static str,
    default_dir: &str,
) -> Option<Location> {
    setting(env, &[setting_name])
        .or_else(|| setting(env, &["XDG_CACHE_HOME"]).map(|root| root.child(default_dir)))
        .or_else(|| under(&env.home, ".cache").map(|root| root.child(default_dir)))
}

fn list_setting(
    env: &Environment,
    key: &'static str,
    separator: Option<char>,
    default: Option<Location>,
) -> Vec<Location> {
    let Some(value) = env.variables.get(key) else {
        return default.into_iter().collect();
    };
    let paths = if let Some(separator) = separator {
        // Conda's comma-separated setting uses UTF-16 here to preserve Windows path spelling.
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let wide = value.encode_wide().collect::<Vec<_>>();
        wide.split(|&unit| u16::try_from(separator).ok() == Some(unit))
            .take(32)
            .map(|part| PathBuf::from(OsString::from_wide(part)))
            .collect::<Vec<_>>()
    } else {
        std::env::split_paths(value).take(32).collect()
    };
    paths
        .into_iter()
        .map(|path| Location::new(path, Source::Environment(key)))
        .collect()
}
