use std::{path::PathBuf, time::Duration};

use crate::detectors::{Confidence, Measurement};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Group {
    JavaScript,
    Java,
    Python,
    Rust,
    DotNet,
    Go,
    Dart,
    Android,
    Editors,
    Testing,
    Models,
}

impl Group {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::JavaScript => "JavaScript",
            Self::Java => "Java",
            Self::Python => "Python",
            Self::Rust => "Rust",
            Self::DotNet => ".NET",
            Self::Go => "Go",
            Self::Dart => "Flutter / Dart",
            Self::Android => "Android",
            Self::Editors => "IDEs and editors",
            Self::Testing => "Testing",
            Self::Models => "Machine learning",
        }
    }
}

macro_rules! tools {
    ($($id:ident => $label:literal, $group:ident);+ $(;)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub(crate) enum Tool { $($id),+ }
        impl Tool {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$id),+];
            pub(crate) const fn name(self) -> &'static str { match self { $(Self::$id => $label),+ } }
            pub(crate) const fn group(self) -> Group { match self { $(Self::$id => Group::$group),+ } }
        }
    };
}
tools! {
    Npm => "npm", JavaScript; Pnpm => "pnpm", JavaScript; Yarn => "Yarn", JavaScript; Bun => "Bun", JavaScript;
    Gradle => "Gradle", Java; Maven => "Maven", Java; Pip => "pip", Python; Uv => "uv", Python;
    Poetry => "Poetry", Python; Conda => "Conda", Python; Cargo => "Cargo", Rust; Nuget => "NuGet", DotNet;
    Go => "Go", Go; Pub => "Dart Pub", Dart; AndroidSdk => "Android SDK", Android;
    AndroidAvd => "Android Virtual Devices", Android; JetBrains => "JetBrains IDEs", Editors;
    VsCode => "Visual Studio Code", Editors; Playwright => "Playwright", Testing;
    Cypress => "Cypress", Testing; Puppeteer => "Puppeteer", Testing;
    HuggingFace => "Hugging Face", Models; Pytorch => "PyTorch Hub", Models;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Kind {
    Cache,
    Dependencies,
    Installed,
    State,
    Protected,
}

impl Kind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Cache => "Cache",
            Self::Dependencies => "Shared dependencies",
            Self::Installed => "Installed tool resource",
            Self::State => "Environment state",
            Self::Protected => "Protected storage",
        }
    }

    pub(crate) const fn risk(self) -> &'static str {
        match self {
            Self::Cache => "Generally regenerable",
            Self::Dependencies => "Review required",
            Self::Installed => "Tool-managed",
            Self::State => "Data-bearing · review",
            Self::Protected => "Protected",
        }
    }

    pub(crate) const fn effects(self) -> (&'static str, &'static str) {
        match self {
            Self::Cache => (
                "A tool may re-download files or repeat CPU-intensive work.",
                "Allow network access, time, and disk space when the tool next needs this cache.",
            ),
            Self::Dependencies => (
                "Multiple projects and offline builds may be affected. Local artifacts or patches may be irreplaceable.",
                "Verify the original package sources, tool version, and availability before attempting to restore the packages.",
            ),
            Self::Installed => (
                "Projects may require this exact resource version even if newer versions are installed.",
                "Use the owning tool to restore the required version and verify compatibility.",
            ),
            Self::State => (
                "This can contain local user data, settings, or resources that are difficult to reproduce.",
                "Review with the owning tool. Keep exports or backups for data that matters.",
            ),
            Self::Protected => (
                "This can contain persistent environment data or work that does not exist elsewhere.",
                "Recovery may depend on a separate backup or export.",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    Default,
    Environment(&'static str),
    Explicit,
}

impl Source {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Default => "Documented or verified Windows location".into(),
            Self::Environment(name) => format!("Environment variable {name}"),
            Self::Explicit => "Explicitly supplied location".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Status {
    Ready,
    Measured,
    Partial,
    Unavailable,
    Skipped,
    Cancelled,
}
impl Status {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Measured => "Measured",
            Self::Partial => "Partial",
            Self::Unavailable => "Unavailable",
            Self::Skipped => "Skipped",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct Identity {
    pub(super) volume: u64,
    pub(super) file: [u8; 16],
    pub(super) canonical: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct Resource {
    pub(crate) id: usize,
    pub(crate) tool: Tool,
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) path: PathBuf,
    pub(crate) drive: char,
    pub(crate) source: Source,
    pub(crate) confidence: Confidence,
    pub(crate) description: &'static str,
    pub(crate) evidence: &'static str,
    pub(crate) status: Status,
    pub(crate) measured: Option<Measurement>,
    pub(crate) skipped_links: u64,
    pub(crate) avd: Option<crate::scanner::AvdSizes>,
    pub(crate) note: String,
    pub(crate) selected: bool,
    pub(super) identity: Option<Identity>,
}

impl Resource {
    pub(crate) fn can_select(&self) -> bool {
        self.identity.is_some()
    }

    pub(crate) fn needs_review(&self) -> bool {
        self.kind != Kind::Cache || self.confidence == Confidence::Contextual
    }

    pub(crate) fn risk(&self) -> &'static str {
        if self.kind == Kind::Cache && self.needs_review() {
            "Review required"
        } else {
            self.kind.risk()
        }
    }

    pub(crate) fn effects(&self) -> (&'static str, &'static str) {
        if self.kind == Kind::Cache && self.needs_review() {
            (
                "Verify the folder contents before considering any changes. A custom location may contain other files.",
                "Confirm the owning tool and the availability of data before planning recovery.",
            )
        } else {
            self.kind.effects()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Inventory {
    pub(crate) resources: Vec<Resource>,
    pub(crate) notices: Vec<String>,
    pub(crate) cancelled: bool,
    pub(crate) truncated: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Progress {
    pub(crate) tools: usize,
    pub(crate) locations: usize,
    pub(crate) processed: usize,
    pub(crate) files: u64,
    pub(crate) elapsed: Duration,
}
