use super::*;

#[cfg(windows)]
fn inventory(root: &Path) -> Registry {
    use crate::scanner::{self, ScanRequest};
    let mut scan = scanner::scan(
        ScanRequest {
            root: root.to_owned(),
        },
        &Cancellation::default(),
        |_| (),
    );
    Registry::from_scan(&mut scan)
}

#[cfg(windows)]
fn fixture() -> tempfile::TempDir {
    // Keep disposable projects outside protected profile and AppData locations.
    let workspace = std::env::current_dir().unwrap();
    let scratch = workspace.join("target").join("cleanup-test-fixtures");
    std::fs::create_dir_all(&scratch).unwrap();
    let fixture = tempfile::Builder::new()
        .prefix("owned-")
        .tempdir_in(scratch)
        .unwrap();
    for (name, bytes) in [
        ("web-雪/package.json", 2),
        ("web-雪/package-lock.json", 3),
        ("web-雪/src/app.js", 17),
        ("web-雪/node_modules/pkg/package.json", 7),
        ("web-雪/node_modules/pkg/payload", 11),
        ("rust/Cargo.toml", 2),
        ("rust/target/.rustc_info.json", 2),
        ("rust/target/debug/executable.exe", 13),
        ("python/pyproject.toml", 2),
        ("python/__pycache__/module.pyc", 9),
        ("python/.venv/pyvenv.cfg", 2),
        ("unknown/target/important", 22),
    ] {
        let path = fixture.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'f'; bytes]).unwrap();
    }
    fixture
}

fn id(session: &Session, name: &str) -> usize {
    session
        .registry()
        .candidates
        .values()
        .find(|item| item.name == name)
        .unwrap()
        .id
}

#[cfg(windows)]
#[test]
fn preview_checks_exact_measured_targets_without_changing_fixture() {
    let fixture = fixture();
    let root = fixture.path();
    let mut session = Session::default();
    session.replace(inventory(root));
    assert_eq!(session.selection().count(), 0);
    assert!(session.request().is_none());
    let node = id(&session, "node_modules");
    let cargo = id(&session, "target");
    let python = id(&session, "__pycache__");
    let protected = id(&session, ".venv");
    assert_eq!(
        session.registry().candidates[&node].eligibility,
        Eligibility::Review
    );
    assert_eq!(
        session.registry().candidates[&cargo].eligibility,
        Eligibility::Review
    );
    assert_eq!(
        session.registry().candidates[&python].eligibility,
        Eligibility::Eligible
    );
    assert_eq!(
        session.registry().candidates[&protected].eligibility,
        Eligibility::Protected
    );
    assert!(!session.set_selected(protected, true));
    assert!(!session.set_selected(99_999, true));
    session.select_visible(&[cargo, node, python]);
    assert_eq!(
        session.selection().map(|item| item.id).collect::<Vec<_>>(),
        [python]
    );
    assert!(session.set_selected(node, true));
    assert!(session.set_selected(cargo, true));
    assert_eq!(session.selected_bytes(), 42);
    let before = [
        "web-雪/package.json",
        "web-雪/package-lock.json",
        "web-雪/src/app.js",
        "web-雪/node_modules/pkg/payload",
        "unknown/target/important",
    ]
    .map(|name| (name, std::fs::read(root.join(name)).unwrap()));
    let plan = preview(
        session.request().unwrap(),
        &Cancellation::default(),
        |_, _| (),
    );
    assert!(plan.rejected.is_empty(), "{:?}", plan.rejected);
    assert_eq!(plan.validated.len(), 3);
    assert!(session.publish(plan));
    for (path, contents) in before {
        assert_eq!(std::fs::read(root.join(path)).unwrap(), contents);
    }
    session.exclude(cargo);
    assert!(session.plan().is_none());
    assert!(!session.set_selected(cargo, true));
    assert!(!session.selected(cargo));
}

#[cfg(windows)]
#[test]
fn rejects_replacements_missing_evidence_and_protected_content() {
    let fixture = fixture();
    let root = fixture.path();
    std::fs::write(root.join("web-雪/node_modules/.env"), b"fixture").unwrap();
    let mut session = Session::default();
    session.replace(inventory(root));
    let node = id(&session, "node_modules");
    let cargo = id(&session, "target");
    let py = id(&session, "__pycache__");
    for id in [node, cargo, py] {
        assert!(session.set_selected(id, true));
    }
    let request = session.request().unwrap();
    std::fs::rename(root.join("rust/target"), root.join("rust/saved")).unwrap();
    std::fs::create_dir(root.join("rust/target")).unwrap();
    std::fs::rename(
        root.join("python/pyproject.toml"),
        root.join("python/old-marker"),
    )
    .unwrap();
    let plan = preview(request, &Cancellation::default(), |_, _| ());
    assert!(plan.validated.is_empty());
    assert_eq!(
        plan.rejected
            .iter()
            .map(|(item, cause)| (item.id, *cause))
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            (node, Rejection::Protected),
            (cargo, Rejection::Changed),
            (py, Rejection::Evidence)
        ])
    );
    assert!(root.join("rust/saved/debug/executable.exe").exists());
}

#[cfg(windows)]
#[test]
fn rejects_reparse_inserted_after_scan_and_report_from_previous_selection() {
    let fixture = fixture();
    let root = fixture.path();
    let outside = root.join("untouched");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("data"), b"important fixture").unwrap();
    let mut session = Session::default();
    session.replace(inventory(root));
    let node = id(&session, "node_modules");
    session.set_selected(node, true);
    let request = session.request().unwrap();
    crate::scanner::tests::junction(&root.join("web-雪/node_modules/new-link"), &outside);
    let result = preview(request, &Cancellation::default(), |_, _| ());
    assert_eq!(result.rejected[0].1, Rejection::Reparse);
    session.set_selected(node, false);
    assert!(!session.publish(result));
    assert_eq!(
        std::fs::read(outside.join("data")).unwrap(),
        b"important fixture"
    );
}

#[cfg(windows)]
#[test]
fn scope_and_protected_paths_fail_closed() {
    assert!(policy::protected_path(Path::new(r"C:\Windows\foo\target")));
    assert!(policy::protected_path(Path::new(
        r"C:\Users\name\AppData\foo\target"
    )));
    assert!(policy::protected_path(Path::new(
        r"C:\Users\name\work\target:stream"
    )));
    assert!(policy::protected_path(Path::new(r"C:\")));
    assert!(!policy::strict_child(
        Path::new(r"C:\ProjectBackups\target"),
        Path::new(r"C:\Project")
    ));
    assert!(!policy::strict_child(
        Path::new(r"C:\Project"),
        Path::new(r"C:\Project")
    ));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Recycle,
    Permanent,
}

#[derive(Debug, PartialEq, Eq)]
enum MockError {
    Unauthorized,
    Stale,
    Unsupported,
    NotAcknowledged,
}

/// Only this test module can supply a recording backend. An approval owns the
/// exact immutable plan and is consumed on the sole call to execute.
struct Approval {
    plan: Plan,
    method: Method,
    policy: u32,
}

fn approve(
    session: &mut Session,
    id: PlanId,
    requested: Method,
    supported: Option<Method>,
    acknowledged: bool,
    policy: u32,
) -> Result<Approval, MockError> {
    let plan = session.plan().ok_or(MockError::Unauthorized)?;
    if plan.id != id || policy != POLICY_VERSION {
        return Err(MockError::Stale);
    }
    if !acknowledged || plan.cancelled || plan.validated.is_empty() {
        return Err(MockError::NotAcknowledged);
    }
    if supported != Some(requested) {
        return Err(MockError::Unsupported);
    }
    Ok(Approval {
        plan: session.plan.take().unwrap(),
        method: requested,
        policy,
    })
}

fn record(
    session: &Session,
    approval: Approval,
    requested: Method,
    stop: &Cancellation,
    mut revalidate: impl FnMut(&Candidate) -> bool,
    mut operation: impl FnMut(Method, &Candidate) -> bool,
) -> Result<Vec<(usize, bool)>, MockError> {
    if (
        session.id,
        session.scan,
        session.revision,
        POLICY_VERSION,
        requested,
    ) != (
        approval.plan.id.session,
        approval.plan.id.scan,
        approval.plan.id.revision,
        approval.policy,
        approval.method,
    ) {
        return Err(MockError::Stale);
    }
    let mut result = Vec::new();
    for candidate in approval.plan.validated {
        if stop.is_cancelled() {
            break;
        }
        if !revalidate(&candidate) {
            break;
        }
        result.push((candidate.id, operation(approval.method, &candidate)));
    }
    Ok(result)
}

#[cfg(windows)]
#[test]
fn aliases_nested_targets_and_excluded_descendants_do_not_expand_plan() {
    let fixture = fixture();
    let mut registry = inventory(fixture.path());
    let root = registry
        .candidates
        .values()
        .find(|item| item.name == "node_modules")
        .unwrap();
    let root_id = root.id;
    let mut alias = root.as_ref().clone();
    alias.id = 50;
    alias.path = alias.path.with_file_name("NODE_MODULES");
    let mut nested = root.as_ref().clone();
    nested.id = 51;
    nested.path.push("nested");
    let identity = nested.identity.as_mut().unwrap();
    identity.1[0] ^= 0xFF;
    identity.2.push("nested");
    registry.candidates.insert(alias.id, Arc::new(alias));
    registry.candidates.insert(nested.id, Arc::new(nested));
    let mut session = Session::default();
    session.replace(registry);
    for id in [root_id, 50, 51] {
        assert!(session.set_selected(id, true));
    }
    assert_eq!(session.selected_bytes(), 18);
    let stop = Cancellation::default();
    let completed = build(
        session.request().unwrap(),
        &stop,
        |_, _| (),
        |_, _, _| Ok(()),
    );
    assert_eq!(completed.validated[0].id, root_id);
    assert_eq!(
        completed
            .rejected
            .iter()
            .map(|(_, reason)| *reason)
            .collect::<Vec<_>>(),
        [Rejection::Overlap, Rejection::Overlap]
    );

    session.exclude(51);
    let completed = build(
        session.request().unwrap(),
        &stop,
        |_, _| (),
        |_, _, _| panic!("overlapping paths reached validator"),
    );
    assert_eq!(completed.validated.len(), 0);
    assert_eq!(
        completed
            .rejected
            .iter()
            .map(|(_, reason)| *reason)
            .collect::<Vec<_>>(),
        [Rejection::Excluded, Rejection::Excluded]
    );
}

#[cfg(windows)]
#[test]
fn cancelling_preview_keeps_completed_validation_and_skips_later_targets() {
    let fixture = fixture();
    let mut session = Session::default();
    session.replace(inventory(fixture.path()));
    session.set_selected(id(&session, "target"), true);
    session.set_selected(id(&session, "__pycache__"), true);
    let stop = Cancellation::default();
    let result = preview(session.request().unwrap(), &stop, |done, _| {
        if done == 1 {
            stop.cancel();
        }
    });
    assert!(result.cancelled);
    assert_eq!(result.validated.len(), 1);
    assert_eq!(result.rejected[0].1, Rejection::Cancelled);
}

#[cfg(windows)]
#[test]
fn recording_backend_stops_when_target_changes_after_approval() {
    let fixture = fixture();
    let root = fixture.path();
    let mut session = Session::default();
    session.replace(inventory(root));
    session.set_selected(id(&session, "target"), true);
    let stop = Cancellation::default();
    let plan = preview(session.request().unwrap(), &stop, |_, _| ());
    let key = plan.id;
    assert_eq!(plan.validated.len(), 1);
    session.publish(plan);
    let approved = approve(
        &mut session,
        key,
        Method::Permanent,
        Some(Method::Permanent),
        true,
        POLICY_VERSION,
    )
    .unwrap();
    std::fs::rename(root.join("rust/target"), root.join("rust/saved")).unwrap();
    std::fs::create_dir(root.join("rust/target")).unwrap();
    let scope = session.registry.scope.as_ref().unwrap();
    let result = record(
        &session,
        approved,
        Method::Permanent,
        &stop,
        |candidate| validate(scope, candidate, &stop).is_ok(),
        |_, _| panic!("the replacement reached the recording backend"),
    )
    .unwrap();
    assert!(result.is_empty());
    assert!(root.join("rust/saved/debug/executable.exe").exists());
}

#[cfg(windows)]
#[test]
fn plan_scopes_single_use_mock_authorization_and_disallows_method_fallback() {
    let fixture = fixture();
    let mut session = Session::default();
    session.replace(inventory(fixture.path()));
    let node = id(&session, "node_modules");
    let py = id(&session, "__pycache__");
    session.set_selected(node, true);
    session.set_selected(py, true);
    let stop = Cancellation::default();
    let plan = build(
        session.request().unwrap(),
        &stop,
        |_, _| (),
        |_, _, _| Ok(()),
    );
    let id = plan.id;
    assert!(session.publish(plan));
    assert_eq!(
        approve(
            &mut session,
            id,
            Method::Recycle,
            None,
            true,
            POLICY_VERSION
        )
        .err(),
        Some(MockError::Unsupported)
    );
    assert_eq!(
        approve(
            &mut session,
            id,
            Method::Permanent,
            Some(Method::Permanent),
            false,
            POLICY_VERSION
        )
        .err(),
        Some(MockError::NotAcknowledged)
    );
    assert_eq!(
        approve(
            &mut session,
            id,
            Method::Recycle,
            Some(Method::Recycle),
            true,
            POLICY_VERSION + 1
        )
        .err(),
        Some(MockError::Stale)
    );
    let approved = approve(
        &mut session,
        id,
        Method::Recycle,
        Some(Method::Recycle),
        true,
        POLICY_VERSION,
    )
    .unwrap();
    assert_eq!(
        approve(
            &mut session,
            id,
            Method::Recycle,
            Some(Method::Recycle),
            true,
            POLICY_VERSION
        )
        .err(),
        Some(MockError::Unauthorized)
    );
    let mut recorded = Vec::new();
    let result = record(
        &session,
        approved,
        Method::Recycle,
        &stop,
        |_| true,
        |method, candidate| {
            recorded.push((method, candidate.id));
            if recorded.len() == 1 {
                stop.cancel();
            }
            false
        },
    )
    .unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, Method::Recycle);
    assert_eq!(result, [(recorded[0].1, false)]);
    assert!(
        fixture
            .path()
            .join("web-雪/node_modules/pkg/payload")
            .exists()
    );
}

#[cfg(windows)]
#[test]
fn replanning_and_rescanning_invalidate_mock_capabilities() {
    let fixture = fixture();
    let mut session = Session::default();
    session.replace(inventory(fixture.path()));
    let node = id(&session, "node_modules");
    session.set_selected(node, true);
    let stop = Cancellation::default();
    let preview = |session: &mut Session| {
        build(
            session.request().unwrap(),
            &stop,
            |_, _| (),
            |_, _, _| Ok(()),
        )
    };
    let plan = preview(&mut session);
    let id = plan.id;
    session.publish(plan);
    let approved = approve(
        &mut session,
        id,
        Method::Permanent,
        Some(Method::Permanent),
        true,
        POLICY_VERSION,
    )
    .unwrap();
    let source = session.source_key();
    session.replace(inventory(fixture.path()));
    assert_ne!(session.source_key(), source);
    assert_eq!(session.selection().count(), 0);
    assert_eq!(
        record(
            &session,
            approved,
            Method::Permanent,
            &stop,
            |_| true,
            |_, _| panic!("stale approval called backend")
        ),
        Err(MockError::Stale)
    );
}
