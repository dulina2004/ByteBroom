use std::{collections::BTreeMap, ffi::OsStr};

use super::Catalog;

pub(crate) struct Comparison {
    pub(crate) previous: u64,
    pub(crate) current: u64,
    pub(crate) counts: [usize; 5],
    pub(crate) changes: Vec<Change>,
    pub(crate) bounded: bool,
}

pub(crate) struct Change {
    pub(crate) name: String,
    pub(crate) delta: i128,
    pub(crate) label: &'static str,
}

pub(crate) fn compare(previous: &Catalog, current: &Catalog) -> Result<Comparison, &'static str> {
    if !previous.hierarchy.complete || !current.hierarchy.complete {
        return Err("Comparison requires two completed, fully measured folder scans.");
    }
    if previous.identity.is_none() || previous.identity != current.identity {
        return Err("Comparison requires two scans of the same verified directory.");
    }
    let a = &previous.hierarchy;
    let b = &current.hierarchy;
    let bounded = a.omitted_count != 0 || b.omitted_count != 0;
    let mut sizes = BTreeMap::<&OsStr, [Option<u64>; 2]>::new();
    for (index, root) in [a, b].into_iter().enumerate() {
        for child in &root.children {
            sizes.entry(&child.name).or_default()[index] = Some(child.measurement.logical_bytes);
        }
    }
    let mut counts = [0; 5];
    let mut changes = Vec::new();
    for (name, [before, after]) in sizes {
        let (category, label) = match (before, after) {
            (Some(before), Some(after)) if before < after => (0, "Increased"),
            (Some(before), Some(after)) if before > after => (1, "Decreased"),
            (Some(_), Some(_)) => (2, "Unchanged"),
            (None, Some(_)) if !bounded => (3, "Newly detected"),
            (Some(_), None) if !bounded => (4, "No longer detected"),
            _ => continue,
        };
        counts[category] += 1;
        if category != 2 {
            changes.push(Change {
                name: name.to_string_lossy().into_owned(),
                label,
                delta: i128::from(after.unwrap_or_default())
                    - i128::from(before.unwrap_or_default()),
            });
        }
    }
    changes.sort_unstable_by(|a, b| {
        b.delta
            .abs()
            .cmp(&a.delta.abs())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Comparison {
        previous: a.measurement.logical_bytes,
        current: b.measurement.logical_bytes,
        counts,
        changes,
        bounded,
    })
}
