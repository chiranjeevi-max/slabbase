# Roadmap

## v1.0 — shipped

- SQLite WAL workspace with an open, integrity-checked single-file format.
- Serialized writer with concurrent read-only replicas, query cancellation, and clean-close WAL truncation.
- Incremental `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` rollups maintained per parent.
- Compact positional tuple-array IPC for table and portal pages.
- BLAKE3 hash-chained audit log with tamper diagnostics, undo/redo, and per-record restore.
- Zero-config `EXPLAIN QUERY PLAN` index advisor with a one-click status-bar fix.
- APFS `clonefile` snapshots with a transactional `VACUUM INTO` fallback.
- Schema builder with typed columns, indexes, and 1:N relations.
- Formula language with acyclic dependency enforcement and token-span renames.
- Virtualized grid, keyset pagination, and FTS5 search.
- Form layouts with embedded portals and live rollups.
- Strict CSV and JSON import with preview; CSV, JSON, and SQL export.
- HTML report preview and vector PDF export.
- macOS shell with System/Light/Dark themes, native Glass vibrancy, seven accents plus custom hex, three densities, native menu bar actions, and an automated design contract.

## v1.1

- Saved relational views with persisted filters, sorts, joined columns, and per-table presets.
- Portal-aware relational `Find` using `EXISTS` child filtering across relations.
- Live SQL console with a read-only default, explicit write opt-in, and audited statements.
- Headless `slab` CLI for open, query, import, export, snapshot, and `Integrity Check` in CI.
- Asset vault plus macOS Quick Look previews for `AttachmentRef` paths.
- Conditional formatting rules for grid and portal cells.
- `.slab` diff for schema, rows, layouts, and audited ranges across two files.
- Row-level data-type conversion migrations with a rollback proof.
- Kanban, gallery, and calendar views through `slab_core::ViewConfig`.
- Airtable CSV and FileMaker XML adapters implementing `slab_import::ImportAdapter`.
- Multi-column sort, group-by, and batch edit for selected rows.
- Split-view side-by-side tables for two related tables or a table beside the schema map.
- Customizable keyboard command chords and saved quick actions in `⌘K`.

## v1.2

- SQLCipher encryption at rest with Touch ID key release from the macOS keychain.
- Schema version snapshots with safe migration diffs that inspect and roll back multi-step refactors as a unit.
- Parameterized saved reports for reusable filtered PDF and invoice presets.
- Branded print studio with logo, palette, and typography controls.
- Declarative local triggers and auto-numbering with transactional audit entries.
- MDM configuration plus SIEM and workspace access-log export for managed fleets.
- Label and barcode templates driven by the existing `slab-print` layout model.
- Field-level redaction policies through `slab_core::PolicyHook`.
- Encrypted attachment vault hooks with keys held in the macOS keychain.

## v2.0

- CRDT sync over `slab_sync::SyncTransport` with HLC-ordered change sets and audit-backed conflict review.
- Parquet bridge for columnar export and import of wide tables.
- Hosted relay and device enrollment for the CRDT transport.
- Optional native SwiftUI shell through UniFFI over the same crates.
- Windows and Linux installers in the verification matrix.

## Known v1.0 limitations

- PDF text is base-14 Helvetica with WinAnsiEncoding; Latin-1 and CP1252 punctuation are exact, common currency and math symbols transliterate to ASCII, and other scripts or emoji render as `?`.
- User identifiers are ASCII (`A-Z`, `a-z`, `0-9`, space, `_`) up to 64 bytes; non-Latin names are rejected rather than quoted.
- Import is strict by default, and `skip_invalid` is the opt-in lossy mode that still reports every skipped row.
- Form PDF fields are single-line and truncate with an ellipsis; multi-paragraph notes do not flow across pages in v1.0.
- Dropping a table that participates in a relation is blocked until the relation is dropped; undo restores the table and then the relation.
- `MIN`/`MAX` rollups over a computed child column refresh lazily for the affected parent; stored child columns use the full incremental path.
- The audit chain is tamper-evident, not tamper-proof against an attacker who rewrites the whole chain or its format metadata; SQLCipher and external anchoring are the v1.2 hardening step.

## Compatibility policy

Workspace format changes are additive within a major version. A newer build may add `_slab_*` tables or columns, but it will not rename or reinterpret existing user tables. An older build opens a file when the file's `format_version` is less than or equal to the build's supported version. A newer format is rejected rather than opened read-only, because silently hiding metadata leads to incomplete forms. Format 2 adds the audit hash scheme columns and is upgraded in one transaction on first open; an older build refuses it.
