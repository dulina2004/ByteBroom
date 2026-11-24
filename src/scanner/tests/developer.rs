use std::{collections::HashSet, fs, os::windows::fs::OpenOptionsExt, path::Path, time::Duration};

use super::{
    Cancellation, ScanOutcome, ScanRequest, file, fixture, junction, run, scan_with_interval,
};
use crate::detectors::{Category, Confidence, Ecosystem, Review};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY};

fn write(root: &Path, relative: &str, bytes: u64) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    file(&path, bytes);
}

#[test]
fn detects_artifacts_with_context_across_supported_ecosystems() {
    let fixture = fixture();
    let root = fixture.path();
    for (relative, bytes) in [
        ("web/package.json", 2),
        ("web/next.config.mjs", 0),
        ("web/angular.json", 0),
        ("web/vite.config.ts", 0),
        ("web/to-save.txt", 10),
        ("web/node_modules/pkg/package.json", 3),
        ("web/node_modules/.vite/out", 4),
        ("web/.next/BUILD_ID", 8),
        ("web/.nuxt/out", 2),
        ("web/.turbo/cache", 3),
        ("web/.parcel-cache/db", 4),
        ("web/.svelte-kit/out", 5),
        ("web/coverage/lcov.info", 6),
        ("web/dist/bundle.js", 7),
        ("web/.angular/cache/entry", 8),
        ("web/.vite/cache", 9),
        ("rust/Cargo.toml", 0),
        ("rust/target/debug/bin", 19),
        ("python/pyproject.toml", 0),
        ("python/.venv/pyvenv.cfg", 5),
        ("python/.venv/bin/python", 3),
        ("python/src/api/__pycache__/api.pyc", 7),
        ("python/.pytest_cache/db", 9),
        ("python/.mypy_cache/db", 11),
        ("python/.ruff_cache/db", 13),
        ("python/.tox/.tox-info.json", 17),
        ("android/settings.gradle.kts", 0),
        ("android/.gradle/state", 23),
        ("android/app/build/intermediates/classes", 29),
        ("java/pom.xml", 0),
        ("java/core/pom.xml", 0),
        ("java/core/target/maven-status/manifest", 31),
        ("dotnet/Project.slnx", 0),
        ("dotnet/client/应用.csproj", 0),
        ("dotnet/client/bin/Debug/main.dll", 37),
        ("dotnet/client/obj/project.assets.json", 41),
        ("flutter/pubspec.yaml", 0),
        ("flutter/.dart_tool/config.json", 43),
        ("flutter/build/flutter_assets/content", 47),
    ] {
        write(root, relative, bytes);
    }
    let result = run(root);
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{:?}",
        result.outcome
    );
    let report = result.developer;
    assert_eq!(report.ecosystem_count, 7);
    assert_eq!(report.projects.len(), 9);
    assert_eq!(report.artifacts.len(), 24);
    let items = [
        (
            "web/node_modules",
            "web",
            Ecosystem::JavaScript,
            Category::Dependencies,
            7,
        ),
        (
            "web/.next",
            "web",
            Ecosystem::JavaScript,
            Category::BuildOutput,
            8,
        ),
        (
            "web/.angular/cache",
            "web",
            Ecosystem::JavaScript,
            Category::Cache,
            8,
        ),
        (
            "web/coverage",
            "web",
            Ecosystem::JavaScript,
            Category::TestOutput,
            6,
        ),
        (
            "rust/target",
            "rust",
            Ecosystem::Cargo,
            Category::BuildOutput,
            19,
        ),
        (
            "python/.venv",
            "python",
            Ecosystem::Python,
            Category::Environment,
            8,
        ),
        (
            "python/src/api/__pycache__",
            "python",
            Ecosystem::Python,
            Category::Cache,
            7,
        ),
        (
            "android/app/build",
            "android",
            Ecosystem::Gradle,
            Category::BuildOutput,
            29,
        ),
        (
            "java/core/target",
            "java/core",
            Ecosystem::Maven,
            Category::BuildOutput,
            31,
        ),
        (
            "dotnet/client/bin",
            "dotnet/client",
            Ecosystem::DotNet,
            Category::BuildOutput,
            37,
        ),
        (
            "dotnet/client/obj",
            "dotnet/client",
            Ecosystem::DotNet,
            Category::BuildOutput,
            41,
        ),
        (
            "flutter/.dart_tool",
            "flutter",
            Ecosystem::Dart,
            Category::Cache,
            43,
        ),
        (
            "flutter/build",
            "flutter",
            Ecosystem::Dart,
            Category::BuildOutput,
            47,
        ),
    ];
    for (path, project, ecosystem, category, bytes) in items {
        let artifact = report
            .artifacts
            .iter()
            .find(|a| a.path == root.join(path))
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(report.projects[artifact.project.0].root, root.join(project));
        assert_eq!(
            (
                artifact.ecosystem,
                artifact.category,
                artifact.measurement.logical_bytes
            ),
            (ecosystem, category, bytes)
        );
        assert!(!artifact.evidence.project_marker.is_empty());
        assert!(!artifact.evidence.artifact_marker.is_empty());
        assert!(artifact.measured_completely);
    }
    let web = report
        .projects
        .iter()
        .find(|p| p.root == root.join("web"))
        .unwrap();
    assert_eq!(web.artifact_count, 10);
    assert_eq!(web.logical_bytes, 59);
    assert!(web.tools.contains(&"Next.js"));
    assert!(
        report
            .artifacts
            .windows(2)
            .all(|pair| pair[0].measurement.logical_bytes >= pair[1].measurement.logical_bytes)
    );
    assert_eq!(
        report.logical_bytes,
        report.projects.iter().map(|p| p.logical_bytes).sum::<u64>()
    );
    assert_eq!(
        report
            .artifacts
            .iter()
            .map(|a| a.id.0)
            .collect::<HashSet<_>>()
            .len(),
        report.artifacts.len()
    );
}

#[test]
fn rejects_generic_names_unsupported_layouts_and_protected_trees() {
    let fixture = fixture();
    for name in [
        "build",
        "target",
        "venv",
        "dist",
        "coverage",
        "bin",
        "obj",
        "node_modules",
    ] {
        write(fixture.path(), &format!("ordinary/{name}/work"), 2);
    }
    assert!(run(fixture.path()).developer.projects.is_empty());
    for path in [
        "js/package.json",
        "js/dist/deliverable",
        "js/coverage/notes",
        "py/requirements.txt",
        "py/venv/personal-file",
        "py/.tox/user-file",
        "gradle/build.gradle",
        "gradle/docs/build/book.pdf",
        "solution/Project.sln",
        "solution/bin/manual",
        "solution/obj/archive",
        "dart/pubspec.yaml",
        "dart/build/image.png",
        "mixed/pom.xml",
        "mixed/Cargo.toml",
        "mixed/target/no-build-evidence",
        ".cargo/registry/src/dep/Cargo.toml",
        ".cargo/registry/src/dep/target/debug/artifact",
        ".git/worktree/package.json",
        ".git/worktree/node_modules/lib",
    ] {
        write(fixture.path(), path, 1);
    }
    let result = run(fixture.path());
    assert_eq!(result.developer.projects.len(), 6);
    assert!(
        result.developer.artifacts.is_empty(),
        "{:?}",
        result.developer.artifacts
    );
}

#[test]
fn separates_workspace_owners_and_counts_outer_artifacts_once() {
    let fixture = fixture();
    let root = fixture.path();
    for (path, bytes) in [
        ("workspace/Cargo.toml", 0),
        ("workspace/package.json", 0),
        ("workspace/target/debug/out", 5),
        ("workspace/target/debug/example/package.json", 2),
        ("workspace/target/debug/example/node_modules/lib", 8),
        ("workspace/node_modules/dep/package.json", 3),
        ("workspace/apps/front/package.json", 0),
        ("workspace/apps/front/node_modules/lib", 11),
        ("workspace/apps/admin/package.json", 0),
        ("workspace/apps/admin/node_modules/lib", 13),
        ("workspace/crates/core/Cargo.toml", 0),
        ("workspace/crates/core/target/debug/bin", 17),
        ("workspace/packages/shared/package.json", 0),
    ] {
        write(root, path, bytes);
    }
    let result = run(root);
    assert_eq!(result.developer.projects.len(), 5);
    assert_eq!(result.developer.artifacts.len(), 5);
    assert_eq!(result.developer.logical_bytes, 59);
    let main = &result.developer.projects[0];
    assert_eq!(main.ecosystems, [Ecosystem::JavaScript, Ecosystem::Cargo]);
    assert_eq!(main.logical_bytes, 18);
    for artifact in result.developer.artifacts {
        assert!(
            artifact
                .path
                .starts_with(&result.developer.projects[artifact.project.0].root)
        );
    }
}

#[test]
fn handles_empty_and_partially_measured_artifacts_and_skips_link_targets() {
    let fixture = fixture();
    let root = fixture.path().join("scope");
    let outside = fixture.path().join("outside");
    write(&outside, "package.json", 0);
    write(&outside, "node_modules/escape", 1000);
    write(&root, "web/package.json", 0);
    write(&root, "web/node_modules/lib", 7);
    write(&root, "web/node_modules/locked/hidden", 500);
    fs::create_dir(root.join("web/.next")).unwrap();
    junction(&root.join("web/node_modules/outside"), &outside);
    let blocked = fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(root.join("web/node_modules/locked"))
        .unwrap();
    let result = run(&root);
    assert!(result.developer.partial);
    assert_eq!(result.progress.inaccessible, 1);
    assert_eq!(result.progress.skipped_links, 1);
    assert_eq!(result.developer.projects.len(), 1);
    let artifacts = &result.developer.artifacts;
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].measurement.logical_bytes, 7);
    assert_eq!(artifacts[0].measurement.files, 1);
    assert_eq!(artifacts[0].measurement.inaccessible, 1);
    assert!(!artifacts[0].measured_completely);
    assert_eq!(artifacts[1].measurement.logical_bytes, 0);
    assert_eq!(artifacts[1].confidence, Confidence::Contextual);
    assert_eq!(artifacts[1].review, Review::Manual);
    assert!(artifacts[1].measured_completely);
    drop(blocked);
    let next = run(&root);
    assert!(!next.developer.partial);
    assert_eq!(next.developer.logical_bytes, 507);
}

#[test]
fn cancellation_preserves_a_partial_developer_result_for_the_current_scan() {
    let fixture = fixture();
    write(fixture.path(), "Cargo.toml", 1);
    for index in 0..40 {
        write(fixture.path(), &format!("target/debug/{index:02}"), 3);
    }
    let cancellation = Cancellation::default();
    let result = scan_with_interval(
        ScanRequest {
            root: fixture.path().into(),
        },
        &cancellation,
        |stats| {
            if stats.files == 12 {
                cancellation.cancel();
            }
        },
        Duration::ZERO,
    );
    assert!(matches!(result.outcome, ScanOutcome::Cancelled));
    assert!(result.developer.partial);
    assert_eq!(result.progress.files, 12);
    assert_eq!(result.developer.artifacts.len(), 1);
    assert!(
        result
            .developer
            .artifacts
            .iter()
            .all(|artifact| !artifact.measured_completely)
    );
    let next = run(fixture.path());
    assert_eq!(next.progress.files, 41);
    assert_eq!(next.developer.logical_bytes, 120);
    assert!(!next.developer.partial);
}
