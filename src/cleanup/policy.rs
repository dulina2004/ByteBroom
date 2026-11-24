use std::path::{Component, Path};

use crate::detectors::{Artifact, Category, Confidence, Ecosystem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rule {
    Node,
    Cargo,
    PythonBytecode,
    PythonTools,
}

impl Rule {
    pub(super) fn markers(self) -> &'static [&'static str] {
        match self {
            Self::Node => &["package.json"],
            Self::Cargo => &["cargo.toml"],
            Self::PythonBytecode | Self::PythonTools => &[
                "pyproject.toml",
                "requirements.txt",
                "pipfile",
                "poetry.lock",
                "setup.py",
                "environment.yml",
            ],
        }
    }
}

fn named_rule(name: &str) -> Option<Rule> {
    match name {
        "node_modules" => Some(Rule::Node),
        "target" => Some(Rule::Cargo),
        "__pycache__" => Some(Rule::PythonBytecode),
        ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => Some(Rule::PythonTools),
        _ => None,
    }
}

#[cfg(windows)]
pub(crate) fn tracked(path: &Path) -> bool {
    named_rule(&super::name(path).to_ascii_lowercase()).is_some()
}

pub(super) fn rule(artifact: &Artifact) -> Option<Rule> {
    let rule = named_rule(&super::name(&artifact.path).to_ascii_lowercase())?;
    let approved = match rule {
        Rule::Node => {
            artifact.ecosystem == Ecosystem::JavaScript
                && artifact.category == Category::Dependencies
                && artifact.evidence.project_marker == "package.json"
        }
        Rule::Cargo => {
            artifact.ecosystem == Ecosystem::Cargo
                && artifact.category == Category::BuildOutput
                && artifact.evidence.project_marker == "Cargo.toml"
        }
        Rule::PythonBytecode | Rule::PythonTools => {
            artifact.ecosystem == Ecosystem::Python && artifact.category == Category::Cache
        }
    };
    (approved && artifact.confidence == Confidence::High).then_some(rule)
}

pub(super) fn strict_child(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
    })
}

pub(super) fn protected_path(path: &Path) -> bool {
    if !path.is_absolute() || path.parent().is_none() {
        return true;
    }
    path.components().any(|part| match part {
        Component::Normal(name) => {
            let Some(name) = name.to_str() else {
                return true;
            };
            let name = name.to_ascii_lowercase();
            name.contains(':')
                || name.ends_with(['.', ' '])
                || matches!(
                    name.as_str(),
                    "windows"
                        | "system32"
                        | "winsxs"
                        | "efi"
                        | "boot"
                        | "recovery"
                        | "system volume information"
                        | "program files"
                        | "program files (x86)"
                        | "programdata"
                        | "appdata"
                        | "$recycle.bin"
                        | "localhistory"
                        | ".android"
                        | ".docker"
                        | "docker"
                        | "wsl"
                        | ".rustup"
                        | ".cargo"
                        | ".m2"
                )
                || sensitive(&name)
        }
        Component::ParentDir | Component::CurDir => true,
        _ => false,
    })
}

fn sensitive(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".gitmodules"
            | ".hg"
            | ".svn"
            | ".ssh"
            | ".gnupg"
            | ".aws"
            | "credentials"
            | "credentials.toml"
            | ".npmrc"
            | ".pypirc"
            | ".netrc"
            | ".env"
            | "id_rsa"
            | "id_ed25519"
            | "local history"
    ) || name.starts_with(".env.")
        || [
            ".pem",
            ".key",
            ".pfx",
            ".p12",
            ".jks",
            ".keystore",
            ".kdbx",
            ".db",
            ".sqlite",
            ".sqlite3",
            ".vhd",
            ".vhdx",
            ".qcow2",
            ".safetensors",
            ".ckpt",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

#[cfg(windows)]
pub(super) fn protected_entry(rule: Rule, name: &str, directory: bool) -> bool {
    if sensitive(name) || name.contains(':') {
        return true;
    }
    if rule == Rule::PythonBytecode {
        return directory || !name.ends_with(".pyc");
    }
    // Dependency manifests and licenses are ordinary inside node_modules. Other
    // project markers inside a cache or compiler output indicate a mixed tree.
    matches!(
        name,
        "cargo.toml"
            | "pom.xml"
            | "pyproject.toml"
            | "pubspec.yaml"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "build.gradle"
            | "build.gradle.kts"
    ) || rule != Rule::Node
        && matches!(
            name,
            "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "cargo.lock"
                | "requirements.txt"
        )
}
