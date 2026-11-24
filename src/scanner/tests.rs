use super::{
    Cancellation, ScanOutcome, ScanRequest, ScanResult, TOP_DIRECTORIES, scan, scan_with_interval,
};
use std::{fs, io, os::windows::fs::OpenOptionsExt, path::Path, process::Command, time::Duration};
use tempfile::TempDir;
mod developer;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY};

fn fixture() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn file(path: &Path, bytes: u64) {
    fs::File::create(path).unwrap().set_len(bytes).unwrap();
}

fn run(root: &Path) -> ScanResult {
    scan(
        ScanRequest { root: root.into() },
        &Cancellation::default(),
        |_| {},
    )
}

fn junction(link: &Path, target: &Path) {
    // Create only inside the generated fixture. Pass paths as data to the fixed command.
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:BYTEBROOM_TEST_LINK -Target $env:BYTEBROOM_TEST_LINK_TARGET | Out-Null"])
        .env("BYTEBROOM_TEST_LINK", link)
        .env("BYTEBROOM_TEST_LINK_TARGET", target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scans_empty_directory_and_reports_root() {
    let fixture = fixture();
    let result = run(fixture.path());
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.root, fixture.path());
    assert_eq!(result.progress.directories, 1);
    assert_eq!(result.progress.files, 0);
    assert_eq!(result.progress.logical_bytes, 0);
    assert!(result.top_directories.is_empty());
}

#[test]
fn counts_nested_sizes_and_does_not_keep_stale_results() {
    let fixture = fixture();
    let root = fixture.path();
    fs::create_dir_all(root.join("alpha/nested")).unwrap();
    fs::create_dir(root.join("beta")).unwrap();
    file(&root.join("first"), 3);
    file(&root.join("second"), 5);
    file(&root.join("alpha/a"), 7);
    file(&root.join("alpha/nested/b"), 11);
    file(&root.join("beta/c"), 13);

    let first = run(root);
    assert!(matches!(first.outcome, ScanOutcome::Completed), "{first:?}");
    assert_eq!(first.progress.files, 5);
    assert_eq!(first.progress.directories, 4);
    assert_eq!(first.progress.logical_bytes, 39);
    assert_eq!(first.progress.inaccessible, 0);
    assert_eq!(first.top_directories[0].name, "alpha");
    assert_eq!(first.top_directories[0].logical_bytes, 18);
    assert_eq!(first.top_directories[1].name, "beta");
    assert_eq!(first.top_directories[1].logical_bytes, 13);

    file(&root.join("third"), 2);
    let second = run(root);
    assert!(
        matches!(second.outcome, ScanOutcome::Completed),
        "{second:?}"
    );
    assert_eq!(second.progress.files, 6);
    assert_eq!(second.progress.logical_bytes, 41);
    assert_eq!(first.top_directories, second.top_directories);
}

#[test]
fn retains_only_the_largest_immediate_directories() {
    let fixture = fixture();
    for size in 0..TOP_DIRECTORIES + 5 {
        let path = fixture.path().join(format!("branch-{size:02}"));
        fs::create_dir(&path).unwrap();
        file(&path.join("leaf"), size as u64);
    }
    let result = run(fixture.path());
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.top_directories.len(), TOP_DIRECTORIES);
    assert_eq!(
        result.top_directories[0].logical_bytes,
        (TOP_DIRECTORIES + 4) as u64
    );
    assert_eq!(result.top_directories.last().unwrap().logical_bytes, 5);
}

#[test]
fn cancels_during_traversal_with_partial_results() {
    let fixture = fixture();
    for index in 0..10 {
        file(&fixture.path().join(index.to_string()), 1);
    }
    let cancellation = Cancellation::default();
    let mut snapshots = Vec::new();
    let result = scan_with_interval(
        ScanRequest {
            root: fixture.path().into(),
        },
        &cancellation,
        |progress| {
            snapshots.push(progress);
            if progress.files == 2 {
                cancellation.cancel();
            }
        },
        Duration::ZERO,
    );
    assert!(matches!(result.outcome, ScanOutcome::Cancelled));
    assert_eq!(result.progress.files, 2);
    assert_eq!(result.progress.logical_bytes, 2);
    assert_eq!(snapshots.first().unwrap().files, 0);
    assert_eq!(snapshots.last().unwrap(), &result.progress);
}

#[test]
fn nonexistent_and_invalid_roots_fail() {
    let fixture = fixture();
    let result = run(&fixture.path().join("missing"));
    assert!(
        matches!(result.outcome, ScanOutcome::Failed(ref error) if error.kind() == io::ErrorKind::NotFound)
    );
    let result = run(Path::new("relative/path"));
    assert!(
        matches!(result.outcome, ScanOutcome::Failed(ref error) if error.kind() == io::ErrorKind::InvalidInput)
    );
    let path = fixture.path().join("plain-file");
    file(&path, 8);
    assert!(matches!(run(&path).outcome, ScanOutcome::Failed(_)));
}

#[test]
fn inaccessible_child_is_counted_and_siblings_continue() {
    let fixture = fixture();
    let blocked = fixture.path().join("blocked");
    fs::create_dir(&blocked).unwrap();
    file(&blocked.join("unseen"), 1000);
    file(&fixture.path().join("visible"), 5);
    let blocked_handle = fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(&blocked)
        .unwrap();

    let result = run(fixture.path());
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.progress.inaccessible, 1);
    assert_eq!(result.progress.files, 1);
    assert_eq!(result.progress.logical_bytes, 5);
    drop(blocked_handle);
    assert_eq!(run(fixture.path()).progress.logical_bytes, 1005);
}

#[test]
fn scans_beyond_regular_win32_path_length() {
    let fixture = fixture();
    let mut deep = fixture.path().to_path_buf();
    for index in 0..16 {
        deep.push(format!("nested-directory-{index:02}"));
        fs::create_dir(&deep).unwrap();
    }
    file(&deep.join("large-path"), 17);
    let result = run(fixture.path());
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.progress.directories, 17);
    assert_eq!(result.progress.logical_bytes, 17);
}

#[test]
fn junctions_are_skipped_and_linked_root_components_are_rejected() {
    let fixture = fixture();
    let root = fixture.path().join("scope");
    let target = fixture.path().join("outside");
    let link = root.join("junction");
    fs::create_dir(&root).unwrap();
    fs::create_dir_all(target.join("nested")).unwrap();
    file(&target.join("nested/hidden"), 1000);
    file(&root.join("inside"), 5);

    junction(&link, &target);

    let result = run(&root);
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.progress.logical_bytes, 5);
    assert_eq!(result.progress.skipped_links, 1);
    assert!(result.top_directories.is_empty());
    assert!(matches!(run(&link).outcome, ScanOutcome::Failed(_)));
    assert!(matches!(
        run(&link.join("nested")).outcome,
        ScanOutcome::Failed(_)
    ));
}

#[test]
fn rejects_a_junction_swapped_in_after_enumeration() {
    use super::windows::{Directory, Entry};

    let fixture = fixture();
    let root = fixture.path().join("scope");
    let child = root.join("child");
    let target = fixture.path().join("outside");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&target).unwrap();
    file(&target.join("hidden"), 1000);

    let mut directory = Directory::open_root(&root, &Cancellation::default())
        .unwrap()
        .unwrap();
    let Some(Entry::Directory(name)) = directory.next(false).unwrap() else {
        panic!("expected the original directory entry");
    };
    fs::rename(&child, root.join("original")).unwrap();
    junction(&child, &target);
    assert!(directory.open_child(&name, true).unwrap().is_none());
}

#[test]
fn symlinks_are_skipped_when_supported_by_the_current_user() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let fixture = fixture();
    let root = fixture.path().join("scope");
    let target = fixture.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&target).unwrap();
    file(&target.join("hidden"), 1000);
    match symlink_dir(&target, root.join("linked-folder")) {
        Ok(()) => {}
        // Windows requires Developer Mode or the symlink privilege.
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!(
                "Symlink creation unavailable for the current user. Junction tests still run."
            );
            return;
        }
        Err(error) => panic!("{error}"),
    }
    symlink_file(target.join("hidden"), root.join("linked-file")).unwrap();
    let result = run(&root);
    assert!(
        matches!(result.outcome, ScanOutcome::Completed),
        "{result:?}"
    );
    assert_eq!(result.progress.logical_bytes, 0);
    assert_eq!(result.progress.skipped_links, 2);
}
