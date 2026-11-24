# ADR 003: Decoupled core crates and a replaceable shell

## Status

Accepted.

## Context

The desktop application is the first surface, not the only one. A future CLI, a SwiftUI shell, or an embedded component in another Rust project should reuse the storage, formula, import, and print logic without dragging in Tauri, React, or macOS APIs. The reverse is also true: the shell should not hold business rules that a second surface would have to reimplement.

## Decision

The workspace is split into six library crates with a strict downward dependency graph, plus one shell crate.

- `slab-core` has no I/O and no third-party runtime dependencies beyond serialization.
- `slab-formula` depends only on `slab-core`.
- `slab-storage`, `slab-import`, `slab-print`, and `slab-sync` depend on `slab-core` and, where required, `slab-formula`. They do not depend on one another.
- `apps/desktop/src-tauri` wires the crates together and owns the worker thread, file dialogs, and IPC.
- The React front end owns view state only. Every read and write is a typed command.

IPC types are generated from Rust with `ts-rs`. A Rust type change that is not regenerated fails `tsc`.

## Consequences

- The core can be compiled for WASM or linked through UniFFI without a refactor.
- Storage cannot call the print renderer directly, so the shell composes them explicitly.
- The shell is replaceable. A SwiftUI client can call the same library functions; only the IPC layer is lost.
- Generated bindings are committed, so a stale binding is visible in review.
- The cost is some conversion code at the boundary and a larger workspace than a single crate.
