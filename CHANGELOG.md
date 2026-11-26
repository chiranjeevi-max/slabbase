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

- The UI audit runs on the Chromium build pinned by `@playwright/test` instead of the runner's system Chrome, and macOS CI and release jobs run on `macos-26`.
- Bump the workspace format to 2 for audit hash scheme 2; an older build refuses the file instead of verifying scheme-2 entries with the scheme-1 algorithm and reporting false tampering.
- Version 1.0.0 is locked across `Cargo.toml`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.
- Pull request macOS CI and Rust CodeQL run only with the `ok-to-test` label; every push gets a quick Ubuntu check and a single `CI gate` status.

### Fixed

- Keep the desktop shell inside the viewport when macOS classic scrollbars are active, and stabilize the macOS release timing and UI audit CI gates.
- Round money multiplication and division once at the currency scale, widen decimal remainders before aligning, compare mixed-scale decimals exactly, and widen addition and subtraction before aligning so a representable mixed-scale sum is not rejected; `₹`/`$` totals no longer lose a cent to double rounding.
- Reject a formula deeper than 128 levels or larger than 16 KiB before recursive consumers see it, drop the expression tree iteratively, and reject an over-limit stored formula at open before any migration so the file is left untouched.
- Parse timestamps with multibyte garbage as a typed error instead of a panic, accept an omitted trailing `Z`, keep `SlabDate` inside the supported 1..=9999 era, and report `iso()` at the clamped instant.
- Preserve select and attachment values on read, so editing another field, importing an attachment column, and the Form Studio select control no longer treat them as plain text. Compare attachment paths with text in formulas and rollups, and restore select and attachment revisions whose audit payload stored them as text.
- Evaluate the record returned by a write inside its transaction; a formula error now rolls the row, audit entry, FTS update, and rollup change back together instead of reporting failure after a commit.
- Capture each cascaded child once when two relations reach it, and keep lookup counts exact through that delete and its undo.
- Reject a foreign SQLite file before changing its journal mode or creating metadata tables.
- Match text filters against the displayed decimal, currency, boolean, and ISO timestamp text, parse typed equality values instead of silently returning no rows, and reject a typed filter with more precision than its column instead of rounding it.
- Exclude computed columns from import auto-mapping and skip them when a source column happens to share their name.
- Preserve empty and whitespace-only text through JSON import (CSV empty fields remain NULL), plus tagged values, currency codes, guarded select options, attachment paths, and a UTF-8 BOM.
- Publish SQL exports through the same scratch-file contract as CSV and JSON, and keep the already-open scratch handle so the dump cannot be redirected through a predictable path.
- Cover the actor, transaction id, and timestamp in the audit hash, migrate a valid scheme-1 chain atomically, and stop backfilling a modern log whose hashes or head anchor were deleted.
- Keep a failed workspace create from closing the workspace already on screen.
- Keep a form's `updated_at` stable across a column rename and its undo, and name the assigned row in an insert's audit summary.
- Draw a portal that shares a grid row with a field, size fallback glyphs correctly, and keep the PDF stream length, HTML subtitle, and truncation notice well-formed.
- Suspend foreign-key enforcement while rebuilding a relation or column, so `DROP TABLE` cannot fire `ON DELETE CASCADE` and silently delete child rows.
- Invalidate derived rollup state on workspace open, use `i128` intermediates for `SUM`/`AVG` replacements, and keep the `MIN`/`MAX` cached count exact across NULL transitions.
- Remove rollup contributions before forgetting parent state during a cascade delete, so no cache row survives for a deleted parent.
- Treat only a wholly unhashed legacy log with no chain anchor as migratable, and backfill it in one transaction, so NULLing hashes cannot launder an altered entry.
- Preserve search, embedded NUL text, late `UNIQUE` indexes, and a fresh audit genesis through a SQL export and restore.
- Reject dangling-symlink snapshot targets, publish `VACUUM INTO` snapshots atomically, and never unlink a concurrently created target.
- Roll back an abandoned import on the next write or clean close instead of wedging the writer in an open transaction.
- Scope query cancellation to one workspace generation and one registration, so a stale interrupt cannot cancel another query.
- Discard stale inspector responses and cancel the delayed row select on table or filter changes, preventing a write against a different table or row.
- Bracket a renamed formula reference when the new column name is not a legal bare identifier.
- Neutralize spreadsheet formula injection, including a leading line feed, and publish CSV/JSON exports atomically so a failed export cannot truncate an existing file.
- Restore the original bare formula spelling when a rename that needed brackets is undone.
- Exclude concurrent read leases from schema-changing commands, so a read never prepares SQL against a schema that was renamed underneath it.
- Create an export's scratch file with the existing destination's permissions, so re-exporting over a private `0600` file never exposes its rows to other users, during the write or after publishing.
- Keep Form Studio edits made while a save is in flight, clamp item position and width to the 12-column grid, and resync the portal relation picker when relations change.
- Keep a New Record draft open when the grid is clicked or double-clicked, and refresh an open inspector when another row is double-clicked.
- Leave a newer selection alone when a delete or insert resolves, and clear the row-loading indicator whenever a page request is discarded.

### Security

- Exact integer minor-unit arithmetic for currency and fixed-precision decimals.
- Stable `SLAB_E_*` error codes and `unwrap`/`expect`/`panic` denial in library crates.
- Zero outbound network requests from the desktop shell, enforced by the Playwright air-gap audit.
- Private vulnerability reporting, Dependabot alerts, automated security fixes, CodeQL, Scorecard, and dependency auditing in CI.
- A log with missing hashes is never repaired automatically; only a wholly unhashed legacy log with no chain anchor migrates, so NULLing every hash cannot launder an altered entry into a fresh chain.
- CSV export prefixes user text that starts with a spreadsheet formula character with an apostrophe, because quoting alone does not stop Excel or Numbers from evaluating it.

### Notes

- `v1.0.0` has not been tagged yet. The first public tag is cut only after the release validation gate and a failure-mode bug-hunt pass.
- Known v1.0 limits and the workspace compatibility policy are documented in [ROADMAP.md](ROADMAP.md). Release operations are documented in [docs/RELEASING.md](docs/RELEASING.md).
