# Cleanup safety

Feature 06 provides a read-only Cleanup preview on direct local Windows volumes that expose file identities. The workspace starts with an empty selection. Use **Details** to inspect a target, **Exclude for this session** to protect a candidate, and **Preview cleanup** to validate the current selection. Selection and previews are held in memory.

## Preview policy

| Resource | Preview policy |
| --- | --- |
| Project-local `__pycache__`, pytest, mypy, and Ruff caches | Allow a preview with verified Python project evidence. Bytecode directories accept only `.pyc` entries. |
| Project-local `node_modules` | Individual review with a verified `package.json`. Reinstalling dependencies can require network access, and exact versions can depend on lockfiles. |
| Cargo project or workspace `target` | Individual review with `Cargo.toml`, strong scan evidence, and a current Cargo output marker. Outputs can include valuable executables or deliverables. |
| Virtual environments, OS and user state, global caches, installed tools, and ambiguous build output | Analysis-only. |

The allowlist applies to preview only. The validator checks original target ownership, volume identity, normalized directory identity, local path scope, and current evidence. It checks all traversed names against the protected-content rules and rejects reparse or offline entries. Embedded repositories, credentials, databases, virtual disks, and unexpected project markers can invalidate an entire candidate. Filename-based evidence has limited coverage of user-created data.

## Validation and execution boundary

The worker checks directory IDs and normalized paths from current handles against the scan, checks that the target is a strict descendant of the authorized scope and project, and reopens the root, project, and target after enumeration. It compares current logical lengths and counts against the scan. Paths, ownership, and file contents can still change concurrently. An equal count and size do not prove that every descendant is the same object.

One request can contain 64 selected candidates. The scan records up to 1,024 project and target identities, and the preview inspects up to 128 nested directories and two million entries per candidate. A validation error or limit excludes the affected candidate. There is no per-file manifest or persistent path log.

**Recycle Bin and permanent deletion are unavailable.** The Shell delete API accepts a Shell item and exposes recycling flags. Its documentation does not establish that the Shell pins the validated filesystem handle and preserves descendant identity across recycling. [Microsoft DeleteItem documentation](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-deleteitem), [operation flags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags)

Windows exposes handle-based disposition for individual objects. A recursive backend still needs to establish that each object remains in the approved scope and is covered by a valid authorization throughout execution. [Microsoft handle disposition documentation](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle)

## Test coverage

Tests use generated workspace-owned fixtures and a recording backend. They cover protected content, overlapping and aliased targets, invalidated previews, target replacement, inserted junctions, cancellation, and single-use approval. The mock exercises execution gating without modifying fixtures. Actual removal, Recycle Bin behavior, storage recovery, and persistent history require a separate safety-gated implementation.
