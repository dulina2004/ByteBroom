use std::{
    io,
    path::{Component, Path, Prefix},
};

use crate::{
    scanner::{
        Cancellation, ScopeIdentity,
        windows::{Directory, Entry},
    },
    windows_fs::ErrorMode,
};

use super::{
    Candidate, Rejection, Scope,
    policy::{self, Rule},
};

const DEPTH_LIMIT: usize = 128;
const ENTRY_LIMIT: u64 = 2_000_000;

pub(super) fn validate(
    scope: &Scope,
    item: &Candidate,
    stop: &Cancellation,
) -> Result<(), Rejection> {
    let _error_mode = ErrorMode::suppress_dialogs().map_err(|_| Rejection::Inaccessible)?;
    let root_id = scope.identity.as_ref().ok_or(Rejection::Scope)?;
    let target_id = item.identity.as_ref().ok_or(Rejection::Scope)?;
    let project_id = item.project_identity.as_ref().ok_or(Rejection::Scope)?;
    let rule = item.rule.ok_or(Rejection::Evidence)?;
    if !policy::strict_child(&target_id.2, &root_id.2)
        || !project_id.2.starts_with(&root_id.2)
        || !policy::strict_child(&target_id.2, &project_id.2)
        || [target_id.0, project_id.0]
            .into_iter()
            .any(|volume| volume != root_id.0)
    {
        return Err(Rejection::Scope);
    }
    if policy::protected_path(&item.path) || policy::protected_path(&target_id.2) {
        return Err(Rejection::Protected);
    }

    let _scope = checked_open(&scope.path, root_id, stop)?;
    let owner = checked_open(&item.project, project_id, stop)?;
    check_marker(owner, rule, stop)?;
    let target = checked_open(&item.path, target_id, stop)?;
    inspect(target, item, rule, stop)?;
    // Re-open from the current namespace after enumeration. This is a stale
    // preview check, not a promise that handles or the subtree are frozen.
    checked_open(&scope.path, root_id, stop)?;
    check_marker(checked_open(&item.project, project_id, stop)?, rule, stop)?;
    checked_open(&item.path, target_id, stop)?;
    Ok(())
}

fn checked_open(
    path: &Path,
    expected: &ScopeIdentity,
    stop: &Cancellation,
) -> Result<Directory, Rejection> {
    checkpoint(stop)?;
    if !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err(Rejection::Scope);
    }
    let directory = Directory::open_inspection(path, stop)
        .map_err(open_error)?
        .ok_or(Rejection::Cancelled)?;
    let (volume, file, path) = directory.identity().map_err(|_| Rejection::Inaccessible)?;
    if file == [0; 16] || ScopeIdentity(volume, file, path) != *expected {
        return Err(Rejection::Changed);
    }
    Ok(directory)
}

fn check_marker(mut project: Directory, rule: Rule, stop: &Cancellation) -> Result<(), Rejection> {
    for _ in 0..20_000 {
        checkpoint(stop)?;
        match project.next_inspection().map_err(read_error)? {
            Some(Entry::NamedFile(name, _))
                if decode(&name).is_ok_and(|name| rule.markers().contains(&name.as_str())) =>
            {
                return Ok(());
            }
            Some(_) => {}
            None => return Err(Rejection::Evidence),
        }
    }
    Err(Rejection::Limit)
}

fn inspect(
    root: Directory,
    item: &Candidate,
    rule: Rule,
    stop: &Cancellation,
) -> Result<(), Rejection> {
    let mut stack = vec![root];
    let (mut files, mut directories, mut bytes) = (0_u64, 1_u64, 0_u64);
    let mut cargo_evidence = false;
    while !stack.is_empty() {
        checkpoint(stop)?;
        if files.saturating_add(directories) > ENTRY_LIMIT {
            return Err(Rejection::Limit);
        }
        let depth = stack.len();
        let entry = stack
            .last_mut()
            .expect("nonempty inspection stack")
            .next_inspection()
            .map_err(read_error)?;
        match entry {
            Some(Entry::Directory(name)) => {
                let decoded = decode(&name)?;
                if policy::protected_entry(rule, &decoded, true) {
                    return Err(Rejection::Protected);
                }
                if depth == DEPTH_LIMIT {
                    return Err(Rejection::Limit);
                }
                let child = stack
                    .last()
                    .expect("nonempty inspection stack")
                    .open_child(&name, true)
                    .map_err(open_error)?
                    .filter(Directory::inspectable)
                    .ok_or(Rejection::Reparse)?;
                directories += 1;
                stack.push(child);
            }
            Some(Entry::NamedFile(name, size)) => {
                let name = decode(&name)?;
                if policy::protected_entry(rule, &name, false) {
                    return Err(Rejection::Protected);
                }
                cargo_evidence |=
                    depth == 1 && matches!(name.as_str(), ".rustc_info.json" | "cachedir.tag");
                files += 1;
                bytes = bytes.checked_add(size).ok_or(Rejection::Limit)?;
            }
            Some(Entry::Link) => return Err(Rejection::Reparse),
            Some(_) => return Err(Rejection::Inaccessible),
            None => {
                stack.pop();
            }
        }
    }
    if rule == Rule::Cargo && !cargo_evidence {
        return Err(Rejection::Evidence);
    }
    if (files, directories, bytes)
        != (
            item.measurement.files,
            item.measurement.directories,
            item.measurement.logical_bytes,
        )
    {
        return Err(Rejection::Changed);
    }
    Ok(())
}

fn checkpoint(stop: &Cancellation) -> Result<(), Rejection> {
    if stop.is_cancelled() {
        Err(Rejection::Cancelled)
    } else {
        Ok(())
    }
}

fn decode(name: &[u16]) -> Result<String, Rejection> {
    String::from_utf16(name)
        .map(|name| name.to_ascii_lowercase())
        .map_err(|_| Rejection::Protected)
}

fn open_error(error: io::Error) -> Rejection {
    match error.kind() {
        io::ErrorKind::NotFound => Rejection::Missing,
        io::ErrorKind::InvalidInput => Rejection::Reparse,
        _ => Rejection::Inaccessible,
    }
}

fn read_error(_: io::Error) -> Rejection {
    Rejection::Inaccessible
}
