use super::*;

#[test]
fn filters_unicode_paths_categories_and_size_without_losing_unknowns() {
    let mut first = Item::synthetic("Étage", "Cache", 500 << 20, String::new());
    first.path = Some(PathBuf::from(r"C:\Dev\雨\cargo"));
    first.group = "Ruby Project".into();
    first.risk = "Review required";
    let zero = Item::synthetic("Empty", "Cache", 0, String::new());
    let mut unknown = Item::synthetic("Unknown", "Installed", 0, String::new());
    unknown.bytes = None;
    let items = [first, zero, unknown];
    assert_eq!(
        matching(&items, "éTAGE", "Review required", Some(500 << 20), 0),
        [0]
    );
    assert_eq!(matching(&items, "雨", "Cache", None, 0), [0]);
    assert_eq!(matching(&items, "ruby PROJECT", "", None, 0), [0]);
    assert_eq!(matching(&items, "", "", None, 1), [1, 0, 2]);
    assert_eq!(matching(&items, "", "", Some(0), 0), [0, 1]);
    assert_eq!(matching(&items, "", "", None, 0), [0, 1, 2]);
    assert_eq!(sum([None, None].into_iter()), None);
    assert_eq!(sum([None, Some(0)].into_iter()), Some(0));
    assert_eq!(sum([Some(u64::MAX), Some(1)].into_iter()), Some(u64::MAX));
}

#[test]
fn dataset_preserves_omitted_size_and_protected_totals() {
    let mut rows = (0..ITEM_LIMIT)
        .map(|id| Item::synthetic(&format!("fixture-{id}"), "Cache", 2, String::new()))
        .collect::<Vec<_>>();
    let mut protected = Item::synthetic("Local History", "Protected", 1, String::new());
    protected.protected = true;
    rows.push(protected);
    let dataset = Dataset::new(rows, true);
    assert_eq!(
        (dataset.rows.len(), dataset.count),
        (ITEM_LIMIT, ITEM_LIMIT + 1)
    );
    assert_eq!(dataset.protected_bytes, Some(1));
    assert_eq!(dataset.omitted, Some(1));
    assert!(dataset.partial);
}

#[cfg(windows)]
#[test]
fn scan_hierarchy_accounts_for_direct_children_omissions_and_depth() {
    use crate::scanner::{self, Cancellation, SUMMARY_DEPTH, ScanRequest, TOP_DIRECTORIES};
    use std::fs;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_owned();
    fs::write(root.join("direct"), [0; 7]).unwrap();
    for index in 0..17 {
        let child = root.join(format!("child-{index:02}"));
        fs::create_dir(&child).unwrap();
        fs::write(child.join("file"), [0; 3]).unwrap();
    }
    let deep = root.join("nested/a/b/c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("data"), [0; 11]).unwrap();
    let mut result = scanner::scan(
        ScanRequest { root: root.clone() },
        &Cancellation::default(),
        |_| (),
    );
    let catalog = Catalog::from_scan(&mut result).unwrap();
    assert_eq!(catalog.hierarchy.direct_bytes, 7);
    assert_eq!(catalog.hierarchy.children.len(), TOP_DIRECTORIES);
    assert_eq!(catalog.hierarchy.omitted_count, 6);
    assert_eq!(catalog.hierarchy.omitted_bytes, 18);
    let rows = catalog.hierarchy.rows(&root);
    assert_eq!(sum(rows.iter().map(|item| item.bytes)), Some(69));
    let (folder, path) = catalog.folder(&[0; SUMMARY_DEPTH]).unwrap();
    assert_eq!(path, root.join("nested/a/b"));
    assert!(!folder.expanded);
    assert_eq!(folder.measurement.logical_bytes, 11);
    assert!(folder.rows(&path).is_empty());
    assert!(catalog.folder(&[TOP_DIRECTORIES]).is_none());
}

#[cfg(windows)]
#[test]
fn compares_only_compatible_fully_measured_scopes() {
    use crate::scanner::{self, Cancellation, ScanRequest};
    use std::fs;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_owned();
    let snapshot = |path: &Path| {
        let mut result = scanner::scan(
            ScanRequest { root: path.into() },
            &Cancellation::default(),
            |_| (),
        );
        Catalog::from_scan(&mut result).unwrap()
    };
    for name in ["growth", "same", "removed", "reduced"] {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("fixture"), [0; 4]).unwrap();
    }
    let previous = snapshot(&root);
    fs::rename(root.join("removed"), root.join("added")).unwrap();
    fs::write(root.join("growth/fixture"), [0; 11]).unwrap();
    fs::write(root.join("reduced/fixture"), [0; 1]).unwrap();
    let mut current = snapshot(&root);
    let comparison = compare(&previous, &current).unwrap();
    assert_eq!(comparison.counts, [1, 1, 1, 1, 1]);
    assert_eq!((comparison.previous, comparison.current), (16, 20));
    assert_eq!(comparison.changes[0].delta, 7);
    assert_eq!(
        compare(&previous, &snapshot(&root.join("growth"))).err(),
        Some("Comparison requires two scans of the same verified directory.")
    );
    current.hierarchy.complete = false;
    assert!(compare(&previous, &current).is_err());
    current.hierarchy.complete = true;
    current.hierarchy.omitted_count = 1;
    let limited = compare(&previous, &current).unwrap();
    assert!(limited.bounded);
    assert_eq!(limited.counts, [1, 1, 1, 0, 0]);
}

#[test]
#[ignore = "repeatable synthetic analysis timing"]
fn benchmark_analysis() {
    use std::time::Instant;
    for count in [1_000, 10_000, 100_000] {
        let items = (0..count)
            .map(|id| {
                Item::synthetic(
                    &format!("Resource {id}"),
                    "Cache",
                    (id as u64).wrapping_mul(268_582_165_773) % (11 << 30),
                    String::new(),
                )
            })
            .collect();
        let start = Instant::now();
        let dataset = Dataset::new(items, false);
        let aggregate = start.elapsed();
        let start = Instant::now();
        let filtered = matching(&dataset.rows, "resource", "Cache", Some(100 << 20), 0);
        let filter_and_sort = start.elapsed();
        let start = Instant::now();
        let tiles = layout(
            filtered
                .iter()
                .map(|&id| (id, dataset.rows[id].bytes.unwrap())),
            usize::MAX,
            700.,
            300.,
        );
        let geometry = start.elapsed();
        assert!(tiles.len() <= super::treemap::TILE_LIMIT);
        println!(
            "{count} items · aggregate {aggregate:?} · filter and sort {filter_and_sort:?} · layout {geometry:?} · {} rows · {} tiles",
            dataset.rows.len(),
            tiles.len()
        );
    }
}
