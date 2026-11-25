# SlabBase

SlabBase is a local-first relational database, form studio, and report workbench for macOS. It stores every workspace as an ordinary SQLite 3 file in WAL mode, so the records remain readable with `sqlite3`, Python, DuckDB, or any other SQLite client. There is no account, no cloud service, and no proprietary container format.

The application is built in Rust with a thin Tauri shell and a React front end. It is licensed under Apache-2.0, with every feature included.

## What it does

- **Relational tables.** Typed columns, foreign keys, indexes, and an interactive schema map.
- **Formula columns.** A small expression language for derived values and parent-level rollups over child rows.
- **Visual forms.** Snap-to-grid form layouts bound to a table, with embedded child portals edited inline.
- **Virtualized grid.** Keyboard-first browsing and editing over keyset-paginated queries.
- **Search.** SQLite FTS5 across all text columns, opened with `Cmd+K`.
- **Audit and undo.** Every write appends a before/after delta to `_slab_audit_log` in the same transaction, and can be undone or redone.
- **Import and export.** Streaming CSV and JSON import with inference and a dry-run preview; CSV, JSON, and SQL export. SQL exports include user tables and SlabBase metadata but omit the audit log and search index; use the backup command for a byte-for-byte snapshot.
- **Print and PDF.** Paginated HTML preview and vector PDF export for records, portals, and filtered tables.
- **Backup.** One-click `VACUUM INTO` snapshots that are consistent while the workspace is open.

## Design commitments

1. **Open storage.** Workspaces are SQLite databases. Metadata lives in `_slab_*` tables; user data lives in plain tables with real foreign key constraints.
2. **Exact arithmetic.** Currency and fixed-precision decimals are stored as integer minor units with an explicit scale. Floating point is never used for money.
3. **Bounded memory.** Grids and portals use keyset pagination and virtualized rendering. Full tables are never loaded.
4. **Acyclic formulas.** Formula dependencies form a DAG. Schema changes that would introduce a cycle are rejected before any mutation.
5. **Transactional audit.** Data and schema writes run in one SQLite transaction that also records the audit delta and the inverse command.
6. **No panics in shipping code.** Library crates deny `unwrap`, `expect`, and `panic`; errors carry stable codes such as `SLAB_E_CYCLE` and `SLAB_E_FK_VIOLATION`.

## Build from source

Prerequisites:

- macOS 13 or newer (Apple silicon or Intel)
- Rust 1.92 or newer with `rustfmt` and `clippy`
- Node.js 22 or newer
- Xcode command line tools

```bash
npm --prefix apps/desktop ci
cargo build --workspace --release
npm --prefix apps/desktop run build
npm --prefix apps/desktop run tauri build
```

Launch the built app with the sample workspace:

```bash
open target/release/bundle/macos/SlabBase.app \
  --args --workspace "$PWD/fixtures/design-studio.slab"
```

For development:

```bash
npm --prefix apps/desktop run tauri dev
```

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p slab-storage --test pagination --release
cargo deny check
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run build
```

The test suite includes property tests for the formula language, randomized integer currency sums, a 100,000-row keyset pagination benchmark, an external `sqlite3` integrity check of a generated workspace, and undo/redo parity across portal edits. The benchmark asserts a 15 ms p95 in release builds and a looser bound in debug builds.

## Workspace format

A `.slab` file is a SQLite 3 database. Inspect it without SlabBase:

```bash
sqlite3 studio.slab '.tables'
sqlite3 studio.slab 'PRAGMA integrity_check; PRAGMA foreign_key_check;'
sqlite3 studio.slab 'SELECT * FROM _slab_tables;'
```

SlabBase sets `foreign_keys = ON`, `journal_mode = WAL`, and `synchronous = NORMAL` on every connection. Closing the application checkpoints the WAL; the database remains valid if the process exits without doing so.

## Crates

| Crate | Responsibility | Depends on |
| --- | --- | --- |
| `slab-core` | Domain types: values, exact decimals, schema, layouts, commands, queries | — |
| `slab-formula` | Parser, typechecker, evaluator, dependency DAG | `slab-core` |
| `slab-storage` | SQLite engine, DDL, pagination, search, audit, undo | `slab-core`, `slab-formula` |
| `slab-import` | Streaming CSV and JSON adapters with inference and preview | `slab-core` |
| `slab-print` | Layout to paginated HTML and PDF | `slab-core` |
| `slab-sync` | Hybrid logical clock, change sets, transport trait | `slab-core` |
| `apps/desktop` | Tauri shell, worker thread, typed IPC, React UI | all of the above |

`slab-core` and `slab-formula` compile without filesystem, network, SQLite, or UI dependencies.

## Status

v1.0 targets macOS. The storage format and core crates are platform independent; Windows and Linux builds are expected to work but are not part of the v1.0 verification matrix.

## License

Apache License 2.0. See `LICENSE` and `NOTICE`.

## Commercial Support & Signed Distributions

All features in this repository are and remain Apache-2.0 open source. Organizations that need more than the source tree can purchase Apple-notarized, MDM-distributable `.pkg` installers, long-term security backports for pinned releases, and response-time support agreements. These offerings cover packaging, signing, and support; they do not gate any product capability. Contact `support@slabbase.app`.
