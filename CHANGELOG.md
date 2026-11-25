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
- Pull request macOS CI and Rust CodeQL run only with the `ok-to-test` label; every push gets a quick Ubuntu check and a single `CI gate` status.

### Fixed

- Keep the desktop shell inside the viewport when macOS classic scrollbars are active, and stabilize the macOS release timing and UI audit CI gates.

### Security

- Exact integer minor-unit arithmetic for currency and fixed-precision decimals.
- Stable `SLAB_E_*` error codes and `unwrap`/`expect`/`panic` denial in library crates.
- Zero outbound network requests from the desktop shell, enforced by the Playwright air-gap audit.
- Private vulnerability reporting, Dependabot alerts, automated security fixes, CodeQL, Scorecard, and dependency auditing in CI.

### Notes

- `v1.0.0` has not been tagged yet. The first public tag is cut only after the release validation gate and an adversarial bug-hunt pass.
- Known v1.0 limits and the workspace compatibility policy are documented in [ROADMAP.md](ROADMAP.md). Release operations are documented in [docs/RELEASING.md](docs/RELEASING.md).
