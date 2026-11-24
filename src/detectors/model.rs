use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Ecosystem {
    JavaScript,
    Cargo,
    Python,
    Gradle,
    Maven,
    DotNet,
    Dart,
}

impl Ecosystem {
    pub(crate) const ALL: [Self; 7] = [
        Self::JavaScript,
        Self::Cargo,
        Self::Python,
        Self::Gradle,
        Self::Maven,
        Self::DotNet,
        Self::Dart,
    ];
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::JavaScript => "JavaScript / TypeScript",
            Self::Cargo => "Rust / Cargo",
            Self::Python => "Python",
            Self::Gradle => "Java / Gradle",
            Self::Maven => "Java / Maven",
            Self::DotNet => ".NET",
            Self::Dart => "Flutter / Dart",
        }
    }
    pub(super) const fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Cache,
    Dependencies,
    Environment,
    BuildOutput,
    TestOutput,
}

impl Category {
    pub(crate) const ALL: [Self; 5] = [
        Self::Cache,
        Self::Dependencies,
        Self::Environment,
        Self::BuildOutput,
        Self::TestOutput,
    ];
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Cache => "Cache",
            Self::Dependencies => "Dependencies",
            Self::Environment => "Environment",
            Self::BuildOutput => "Build output",
            Self::TestOutput => "Test output",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confidence {
    High,
    Contextual,
}

impl Confidence {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Contextual => "Contextual",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Review {
    Cache,
    Regenerable,
    Manual,
}

impl Review {
    pub(crate) const ALL: [Self; 3] = [Self::Cache, Self::Regenerable, Self::Manual];
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Cache => "Regenerable cache",
            Self::Regenerable => "Regenerable with setup",
            Self::Manual => "Review required",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProjectId(pub(crate) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactId(pub(crate) usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Measurement {
    pub(crate) logical_bytes: u64,
    pub(crate) files: u64,
    pub(crate) directories: u64,
    pub(crate) inaccessible: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Evidence {
    pub(crate) project_marker: &'static str,
    pub(crate) artifact_marker: &'static str,
}

#[derive(Debug)]
pub(crate) struct Project {
    pub(crate) root: PathBuf,
    pub(crate) ecosystems: Vec<Ecosystem>,
    pub(crate) tools: Vec<&'static str>,
    pub(crate) logical_bytes: u64,
    pub(crate) artifact_count: usize,
    pub(crate) incomplete: bool,
}

impl Project {
    pub(crate) fn name(&self) -> std::borrow::Cow<'_, str> {
        self.root
            .file_name()
            .unwrap_or(self.root.as_os_str())
            .to_string_lossy()
    }
}

#[derive(Debug)]
pub(crate) struct Artifact {
    pub(crate) id: ArtifactId,
    pub(crate) path: PathBuf,
    pub(crate) project: ProjectId,
    pub(crate) ecosystem: Ecosystem,
    pub(crate) category: Category,
    pub(crate) confidence: Confidence,
    pub(crate) review: Review,
    pub(crate) description: &'static str,
    pub(crate) regeneration: &'static str,
    pub(crate) warning: &'static str,
    pub(crate) evidence: Evidence,
    pub(crate) measurement: Measurement,
    pub(crate) measured_completely: bool,
}

impl Artifact {
    pub(crate) fn name(&self) -> std::borrow::Cow<'_, str> {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Results {
    pub(crate) projects: Vec<Project>,
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) logical_bytes: u64,
    pub(crate) ecosystem_count: u32,
    pub(crate) partial: bool,
    pub(crate) truncated: bool,
}
