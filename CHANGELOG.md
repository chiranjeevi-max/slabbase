# Changelog

All notable changes to SlabBase are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Open SQLite WAL workspaces with ordinary user tables, real foreign keys, and namespaced `_slab_*` metadata.
- Typed schema builder with columns, indexes, 1:N relations, and a monochrome relation graph.
- Incremental `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` rollups maintained per parent.
- Acyclic formula language with a hand-written parser, typechecker, evaluator, and dependency DAG.
- Virtualized grid with keyset pagination, multi-column sort, filters, and FTS5 search.
- Form Studio with a 12-column drafting grid, field palette, item inspector, and embedded child portals.
- BLAKE3 hash-chained audit log with undo/redo, per-record revision history, and audited restore.
- Zero-config `EXPLAIN QUERY PLAN` index advisor with a one-click status-bar fix.
- APFS copy-on-write snapshots with a transactional `VACUUM INTO` fallback.
- Streaming CSV and JSON import with inference, preview, and strict row validation.
- CSV, JSON, and SQL export; paginated HTML preview and vector PDF reports.
- Native macOS shell with System/Light/Dark themes, Solid/Glass vibrancy, seven accents plus custom hex, and three densities.
- `cargo-auditable`-compatible release binaries and compact tuple-array IPC frames.
- GitHub CI, CodeQL, Scorecard, nightly mutation testing, and a draft release pipeline with CycloneDX SBOMs and Sigstore attestations.

### Changed

- Version 1.0.0 is locked across `Cargo.toml`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.

### Fixed

- Keep the desktop shell inside the viewport when macOS classic scrollbars are active, and stabilize the macOS release timing and UI audit CI gates.
- Suspend foreign-key enforcement while rebuilding a relation or column, so `DROP TABLE` cannot fire `ON DELETE CASCADE` and silently delete child rows.
- Invalidate derived rollup state on workspace open, use `i128` intermediates for `SUM`/`AVG` replacements, and keep the `MIN`/`MAX` cached count exact across NULL transitions.
- Remove rollup contributions before forgetting parent state during a cascade delete, so no cache row survives for a deleted parent.
- Treat only a wholly unhashed audit log as legacy, and backfill it in one transaction, so a partially NULLed log is reported as tampered instead of laundered.
- Preserve search, embedded NUL text, late `UNIQUE` indexes, and a fresh audit genesis through a SQL export and restore.
- Reject dangling-symlink snapshot targets, publish `VACUUM INTO` snapshots atomically, and never unlink a concurrently created target.
- Roll back an abandoned import on the next write or clean close instead of wedging the writer in an open transaction.
- Scope query cancellation to one workspace generation and one registration, so a stale interrupt cannot cancel another query.
- Discard stale inspector responses and cancel the delayed row select on table or filter changes, preventing a write against a different table or row.
- Bracket a renamed formula reference when the new column name is not a legal bare identifier.
- Neutralize spreadsheet formula injection in CSV export, and validate the target table before truncating a JSON export.
- Exclude concurrent read leases from schema-changing commands, so a read never prepares SQL against a schema that was renamed underneath it.

### Security

- Exact integer minor-unit arithmetic for currency and fixed-precision decimals.
- Stable `SLAB_E_*` error codes and `unwrap`/`expect`/`panic` denial in library crates.
- Zero outbound network requests from the desktop shell, enforced by the Playwright air-gap audit.
- Private vulnerability reporting, Dependabot alerts, automated security fixes, CodeQL, Scorecard, and dependency auditing in CI.
- A partially hashed audit log is never repaired automatically; only a wholly legacy log migrates, so NULLing one hash cannot launder an altered entry into a fresh chain.
- CSV export prefixes user text that starts with a spreadsheet formula character with an apostrophe, because quoting alone does not stop Excel or Numbers from evaluating it.

### Notes

- `v1.0.0` has not been tagged yet. The first public tag is cut only after the release validation gate and an adversarial bug-hunt pass.
- Known v1.0 limits and the workspace compatibility policy are documented in [ROADMAP.md](ROADMAP.md). Release operations are documented in [docs/RELEASING.md](docs/RELEASING.md).
