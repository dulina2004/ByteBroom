use std::{
    fs,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use super::*;

fn setup() -> (tempfile::TempDir, Environment) {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("profile-雪");
    let local = home.join("Local");
    let roaming = home.join("Roaming");
    let temp = local.join("Temp");
    for directory in [&home, &local, &roaming, &temp] {
        fs::create_dir_all(directory).unwrap();
    }
    let env = Environment {
        home: Some(home),
        local: Some(local),
        roaming: Some(roaming),
        temp: Some(temp),
        ..Environment::default()
    };
    (fixture, env)
}

fn create(base: &Option<PathBuf>, relative: &str) -> PathBuf {
    let path = base.as_ref().unwrap().join(relative);
    fs::create_dir_all(&path).unwrap();
    path
}

fn discover_test(env: &Environment, tool: Option<Tool>, custom: &[CustomRoot]) -> Inventory {
    discover_with(env, tool, custom, &Cancellation::default(), |_| ())
}

fn assigned<'a>(inventory: &'a Inventory, tool: Tool, name: &str) -> &'a Resource {
    inventory
        .resources
        .iter()
        .find(|row| row.tool == tool && row.name.contains(name))
        .unwrap_or_else(|| {
            panic!(
                "missing {} / {name}: {:?}",
                tool.name(),
                inventory.resources
            )
        })
}

#[test]
fn discovers_documented_package_paths_without_opening_protected_homes() {
    let (_fixture, env) = setup();
    for path in [
        "npm-cache/_cacache",
        "pnpm/store/v10",
        "Yarn/Cache",
        "pip/Cache/http-v2",
        "uv/cache",
        "pypoetry/Cache/artifacts",
        "pypoetry/Cache/virtualenvs",
        "NuGet/v3-cache",
        "NuGet/plugins-cache",
        "go-build",
        "Pub/Cache/hosted",
    ] {
        create(&env.local, path);
    }
    for path in [
        ".gradle/caches/modules-2",
        ".gradle/caches/build-cache-1",
        ".gradle/wrapper/dists",
        ".gradle/jdks",
        ".gradle/init.d",
        ".m2/repository",
        ".cargo/registry/cache",
        ".cargo/registry/src",
        ".cargo/bin",
        ".cargo/git/db",
        ".cargo/git/checkouts",
        ".conda/pkgs",
        ".conda/envs",
        ".nuget/packages",
        "go/pkg/mod",
        ".bun/install/cache",
        ".yarn/berry/cache",
    ] {
        create(&env.home, path);
    }
    create(&env.temp, "NuGetScratch");
    let found = discover_test(&env, None, &[]);
    assert_eq!(assigned(&found, Tool::Npm, "Download").kind, Kind::Cache);
    assert_eq!(assigned(&found, Tool::Pnpm, "v10").kind, Kind::Dependencies);
    assert_eq!(
        assigned(&found, Tool::Gradle, "Dependency").kind,
        Kind::Dependencies
    );
    assert_eq!(
        assigned(&found, Tool::Gradle, "Wrapper").kind,
        Kind::Installed
    );
    assert_eq!(
        assigned(&found, Tool::Maven, "repository").kind,
        Kind::Dependencies
    );
    assert_eq!(
        assigned(&found, Tool::Poetry, "environments").kind,
        Kind::State
    );
    assert_eq!(
        assigned(&found, Tool::Conda, "environments").kind,
        Kind::State
    );
    assert_eq!(assigned(&found, Tool::Nuget, "Temporary").kind, Kind::State);
    for tool in [
        Tool::Yarn,
        Tool::Bun,
        Tool::Pip,
        Tool::Uv,
        Tool::Cargo,
        Tool::Nuget,
        Tool::Go,
        Tool::Pub,
    ] {
        assert!(found.resources.iter().any(|item| item.tool == tool));
    }
    assert!(found.resources.iter().all(|row| !row.path.ends_with("bin")
        && !row.path.ends_with("init.d")
        && !row.path.ends_with(".gradle")
        && !row.path.ends_with(".m2")));
}

#[test]
fn discovers_android_ide_testing_and_model_resources_conservatively() {
    let (_fixture, env) = setup();
    for path in [
        "Android/Sdk/platforms/android-36",
        "Android/Sdk/build-tools/36.0.0",
        "Android/Sdk/system-images/android-36",
        "Android/Sdk/ndk/28.0.0",
        "Android/Sdk/cmake/4.0.0",
        "Android/Sdk/platform-tools",
        "JetBrains/RustRover2026.1/caches",
        "JetBrains/RustRover2026.1/LocalHistory",
        "Google/AndroidStudio2026.1/log",
        "ms-playwright",
        "Cypress/Cache",
    ] {
        create(&env.local, path);
    }
    let avd = create(&env.home, ".android/avd/test.avd/snapshots");
    fs::write(avd.join("memory"), [0; 13]).unwrap();
    fs::write(avd.parent().unwrap().join("userdata-qemu.img"), [0; 21]).unwrap();
    for path in [
        ".vscode/extensions",
        ".cache/puppeteer/chrome",
        ".cache/huggingface/hub",
        ".cache/huggingface/datasets",
        ".cache/torch/hub",
    ] {
        create(&env.home, path);
    }
    for path in [
        "Code/User/workspaceStorage",
        "Code/Backups",
        "Code - Insiders/CachedData",
    ] {
        create(&env.roaming, path);
    }
    let found = discover_test(&env, None, &[]);
    assert_eq!(
        assigned(&found, Tool::AndroidSdk, "android-36").kind,
        Kind::Installed
    );
    assert_eq!(
        assigned(&found, Tool::AndroidAvd, "test.avd").kind,
        Kind::Protected
    );
    assert_eq!(
        assigned(&found, Tool::JetBrains, "Local History").kind,
        Kind::Protected
    );
    assert_eq!(
        assigned(&found, Tool::VsCode, "Workspace").kind,
        Kind::State
    );
    assert_eq!(
        assigned(&found, Tool::VsCode, "backups").kind,
        Kind::Protected
    );
    for tool in [
        Tool::Playwright,
        Tool::Cypress,
        Tool::Puppeteer,
        Tool::HuggingFace,
        Tool::Pytorch,
    ] {
        assert!(found.resources.iter().any(|row| row.tool == tool));
    }
    let inventory = discover_test(&env, Some(Tool::AndroidAvd), &[]);
    let result = measure(inventory, &Cancellation::default(), |_| ());
    let row = assigned(&result, Tool::AndroidAvd, "test.avd");
    assert_eq!(row.measured.unwrap().logical_bytes, 34);
    let sizes = row.avd.unwrap();
    assert_eq!(
        (sizes.disk_files, sizes.disk_bytes, sizes.snapshot_bytes),
        (1, 21, Some(13))
    );
}

#[test]
fn handles_overrides_missing_paths_and_multiple_explicit_stores() {
    let (fixture, mut env) = setup();
    assert!(discover_test(&env, None, &[]).resources.is_empty());
    let npm = fixture.path().join("Custom npm 缓存");
    fs::create_dir_all(npm.join("_cacache")).unwrap();
    create(&env.local, "npm-cache/_cacache");
    env.variables
        .insert("npm_config_cache", npm.clone().into_os_string());
    let found = discover_test(&env, Some(Tool::Npm), &[]);
    assert_eq!(found.resources.len(), 1);
    assert_eq!(found.resources[0].path, npm.join("_cacache"));
    assert_eq!(
        found.resources[0].source,
        Source::Environment("npm_config_cache")
    );
    assert!(!found.resources[0].selected);
    assert_eq!(found.resources[0].risk(), "Review required");

    let mut custom = Vec::new();
    for name in ["Store A", "Store B"] {
        let store = fixture.path().join(name);
        fs::create_dir_all(store.join("v3")).unwrap();
        custom.push(CustomRoot {
            tool: Tool::Pnpm,
            path: store,
        });
    }
    let found = discover_test(&env, Some(Tool::Pnpm), &custom);
    assert_eq!(found.resources.len(), 2);
    assert!(
        found
            .resources
            .iter()
            .all(|row| row.source == Source::Explicit && !row.selected)
    );
}

#[test]
fn rejects_remote_system_and_broad_overrides_without_fallback() {
    let (_fixture, mut env) = setup();
    create(&env.local, "npm-cache/_cacache");
    create(&env.local, "uv/cache");
    for input in [
        r"\\server\share\cache",
        r"\\?\UNC\server\share\cache",
        r"C:\",
        r"C:\work\..\wide",
        r"C:\users\.ssh",
        r"C:\Windows\System32",
    ] {
        assert_eq!(path_drive(Path::new(input)), None);
    }
    for input in [
        r"\\127.0.0.1\share\cache".into(),
        env.home.clone().unwrap().into_os_string(),
    ] {
        env.variables.insert("npm_config_cache", input);
        let found = discover_test(&env, None, &[]);
        assert!(found.resources.iter().all(|item| item.tool != Tool::Npm));
        assert!(found.notices.iter().any(|notice| notice.contains("npm")));
        assert_eq!(assigned(&found, Tool::Uv, "Package").status, Status::Ready);
    }
}

#[test]
fn measures_selected_roots_once_and_preserves_unknown_sizes() {
    let (fixture, env) = setup();
    let outer = fixture.path().join("outer");
    let inner = outer.join("inner");
    let empty = fixture.path().join("empty");
    for path in [&inner, &empty] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(outer.join("root-file"), [0; 7]).unwrap();
    fs::write(inner.join("leaf"), [0; 11]).unwrap();
    fs::hard_link(inner.join("leaf"), inner.join("second-name")).unwrap();
    let roots = [&outer, &inner, &empty, &outer].map(|path| CustomRoot {
        tool: Tool::Maven,
        path: path.clone(),
    });
    let mut found = discover_test(&env, Some(Tool::Maven), &roots);
    assert_eq!(found.resources.len(), 3);
    for item in &mut found.resources {
        item.selected = item.path != empty;
    }
    let first = measure(found, &Cancellation::default(), |_| ());
    let get = |path: &Path| first.resources.iter().find(|row| row.path == path).unwrap();
    assert_eq!(get(&outer).measured.unwrap().logical_bytes, 7);
    assert_eq!(get(&inner).status, Status::Measured);
    assert_eq!(get(&inner).measured.unwrap().logical_bytes, 22);
    assert!(get(&empty).measured.is_none());

    let mut found = first;
    for item in &mut found.resources {
        item.selected = item.path != inner;
    }
    let second = measure(found, &Cancellation::default(), |_| ());
    let resource = second
        .resources
        .iter()
        .find(|item| item.path == empty)
        .unwrap();
    assert_eq!(resource.status, Status::Measured);
    assert_eq!(resource.measured.unwrap().logical_bytes, 0);
    let get = |path: &Path| {
        second
            .resources
            .iter()
            .find(|row| row.path == path)
            .unwrap()
    };
    assert_eq!(get(&outer).measured.unwrap().logical_bytes, 7);
    assert_eq!(get(&inner).status, Status::Skipped);
    assert!(get(&inner).measured.is_none());
}

#[test]
fn cancellation_and_changed_directories_never_substitute_targets() {
    let (_fixture, env) = setup();
    let path = create(&env.home, ".m2/repository");
    let stop = Cancellation::default();
    stop.cancel();
    assert!(discover_with(&env, None, &[], &stop, |_| ()).cancelled);
    let found = discover_test(&env, Some(Tool::Maven), &[]);
    let cancelled = measure(found, &stop, |_| ());
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.resources[0].status, Status::Cancelled);
    assert!(cancelled.resources[0].measured.is_none());

    let found = discover_test(&env, Some(Tool::Maven), &[]);
    fs::rename(&path, path.with_file_name("old-repository")).unwrap();
    fs::create_dir(&path).unwrap();
    let changed = measure(found, &Cancellation::default(), |_| ());
    assert_eq!(changed.resources[0].status, Status::Unavailable);
    assert!(changed.resources[0].measured.is_none());
}

#[test]
fn linked_roots_and_blocked_children_keep_unknown_and_partial_sizes_separate() {
    use crate::scanner::tests::junction;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    };

    let (fixture, env) = setup();
    let outer = create(&env.home, ".m2/repository");
    let blocked = create(&env.home, ".m2/repository/blocked");
    let outside = fixture.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("private"), [0; 77]).unwrap();
    fs::write(blocked.join("hidden"), [0; 33]).unwrap();
    fs::write(outer.join("visible"), [0; 5]).unwrap();
    junction(&outer.join("outside-link"), &outside);
    let alias = fixture.path().join("alias");
    junction(&alias, &outside);
    let invalid = discover_test(
        &env,
        Some(Tool::Maven),
        &[CustomRoot {
            tool: Tool::Maven,
            path: alias,
        }],
    );
    assert!(invalid.resources.is_empty());
    assert!(!invalid.notices.is_empty());

    let found = discover_test(&env, Some(Tool::Maven), &[]);
    let blocked_handle = fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(&blocked)
        .unwrap();
    let partial = measure(found, &Cancellation::default(), |_| ());
    let row = &partial.resources[0];
    assert_eq!(row.status, Status::Partial);
    assert_eq!(row.measured.unwrap().logical_bytes, 5);
    assert_eq!(row.measured.unwrap().inaccessible, 1);
    drop(blocked_handle);
    let completed = measure(partial, &Cancellation::default(), |_| ());
    assert_eq!(completed.resources[0].measured.unwrap().logical_bytes, 38);
}

#[test]
fn cancellation_checkpoints_preserve_completed_measurements_and_bound_discovery() {
    let (_fixture, env) = setup();
    for index in 0..(MAX_LOCATIONS + 2) {
        create(&env.local, &format!("Android/Sdk/ndk/{index}.0"));
    }
    let stop = Cancellation::default();
    let interrupted = discover_with(&env, Some(Tool::AndroidSdk), &[], &stop, |_| stop.cancel());
    assert!(interrupted.cancelled);
    let found = discover_test(&env, Some(Tool::AndroidSdk), &[]);
    assert!(found.truncated);
    assert_eq!(found.resources.len(), MAX_LOCATIONS);
    let stop = Cancellation::default();
    let result = measure(found, &stop, |progress| {
        if progress.processed == 1 {
            stop.cancel();
        }
    });
    assert!(result.cancelled);
    assert_eq!(result.resources[0].measured.unwrap().logical_bytes, 0);
    assert_eq!(result.resources[1].status, Status::Cancelled);
    assert!(result.resources[1].measured.is_none());
}

#[test]
fn plans_drive_stores_and_split_overrides_without_probing_other_volumes() {
    let mut env = Environment {
        drives: vec!['D', 'E'],
        ..Environment::default()
    };
    env.variables
        .insert("CONDA_PKGS_DIRS", r"D:\shared\pkgs,E:\packages\雪".into());
    env.variables
        .insert("CONDA_ENVS_PATH", r"D:\conda\envs;E:\other\envs".into());
    let stores = providers::plans(&env, &[], Some(Tool::Pnpm));
    assert_eq!(
        stores
            .iter()
            .map(|plan| path_drive(&plan.location.path))
            .collect::<Vec<_>>(),
        vec![Some('D'), Some('E')]
    );
    let conda = providers::plans(&env, &[], Some(Tool::Conda));
    assert_eq!(conda.len(), 4);
    assert_eq!(conda[1].location.path, Path::new(r"E:\packages\雪"));
    assert_eq!(conda[3].definition.kind, Kind::State);
    env.variables.insert("PNPM_HOME", r"D:\Tools\pnpm".into());
    let pnpm = providers::plans(&env, &[], Some(Tool::Pnpm));
    assert_eq!(pnpm[0].location.path, Path::new(r"D:\Tools\pnpm\store"));
    assert_eq!(pnpm[0].location.source, Source::Environment("PNPM_HOME"));
}

#[test]
fn project_and_global_views_keep_overlapping_totals_independent() {
    let (fixture, env) = setup();
    let project = fixture.path().join("Web");
    let dependencies = project.join("node_modules");
    fs::create_dir_all(&dependencies).unwrap();
    fs::write(project.join("package.json"), []).unwrap();
    fs::write(dependencies.join("fixture-dependency"), [0; 19]).unwrap();
    let project_scan = scanner::scan(
        scanner::ScanRequest { root: project },
        &Cancellation::default(),
        |_| (),
    );
    assert_eq!(project_scan.developer.artifacts.len(), 1);
    assert_eq!(
        project_scan.developer.artifacts[0]
            .measurement
            .logical_bytes,
        19
    );
    let mut preview = discover_test(
        &env,
        Some(Tool::Yarn),
        &[CustomRoot {
            tool: Tool::Yarn,
            path: dependencies,
        }],
    );
    preview.resources[0].selected = true;
    let global_scan = measure(preview, &Cancellation::default(), |_| ());
    assert_eq!(global_scan.resources[0].measured.unwrap().logical_bytes, 19);
}
