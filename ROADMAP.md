# Roadmap

## v1.0 — shipped

- SQLite WAL workspace, open format, integrity-checked.
- Reader/writer pool separation with query cancellation and clean-close WAL truncation.
- Incremental view maintenance for `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` rollups.
- Compact positional tuple-array IPC framing for table and portal pages.
- BLAKE3 hash-chained audit log, tamper diagnostics, and per-record time travel with restore.
- Zero-config `EXPLAIN QUERY PLAN` index advisor.
- Instant APFS `clonefile` snapshots with `VACUUM INTO` fallback.
- Schema builder with typed columns and 1:N relations.
- Formula language with acyclic dependency enforcement.
- Virtualized grid, keyset pagination, FTS5 search.
- Form layouts with embedded portals and live rollups.
- Transactional audit log with undo and redo.
- CSV and JSON import with preview; CSV, JSON, and SQL export.
- HTML report preview and vector PDF export.
- Atomic snapshots and backups.
- macOS desktop shell, light and dark studio themes.

## v1.1

- Saved relational views: persisted filters, sorts, joined relation columns, and per-table presets.
- Live SQL console with a read-only default, explicit write opt-in, and every statement audited.
- Headless `slab` CLI for open, query, import, export, snapshot, and `Integrity Check` in CI.
- Asset vault plus macOS Quick Look previews for `AttachmentRef` paths.
- Conditional formatting rules for grid and portal cells.
- `.slab` diff: schema, rows, layouts, and audit ranges compared across two workspace files.
- Arbitrary column data-type conversion migrations. v1.0 supports renaming, adding, and dropping columns, but changing an existing stored type is deliberately deferred because it requires a row-level conversion plan and rollback proof.
- Additional view types through `slab_core::ViewConfig`: kanban, gallery, calendar.
- Airtable CSV and FileMaker XML adapters implementing `slab_import::ImportAdapter`.
- Multi-column sort and group-by in the grid.
- Batch edit for selected rows.

## v1.2

- SQLCipher encryption at rest with Touch ID key release from the macOS keychain.
- Branded print studio: logo, palette, typography, and saved report templates in `slab-print`.
- MDM configuration and SIEM audit export for managed fleets.
- Label and barcode templates in `slab-print`, driven by the same layout model.
- Field-level redaction policies through `slab_core::PolicyHook`.
- Encrypted attachment vault hooks, with keys held in the macOS keychain.
- Workspace-level access log export.

## v2.0

- CRDT sync over `slab_sync::SyncTransport`, with HLC-ordered change sets and conflict review backed by the audit deltas.
- Parquet bridge for columnar export and import of wide tables.
- Hosted relay and device enrollment for the CRDT transport.
- Optional native SwiftUI shell through UniFFI over the same crates.
- Windows and Linux installers in the verification matrix.

## Known v1.0 limitations

- PDF text is base-14 Helvetica with WinAnsiEncoding. Latin-1 and CP1252 punctuation are exact; common currency and math symbols transliterate to ASCII; other scripts and emoji render as `?`. Shipping a Unicode font is deferred.
- User identifiers are ASCII (`A-Z`, `a-z`, `0-9`, space, `_`) up to 64 bytes. Non-Latin table and column names are rejected rather than quoted, which keeps formula source and SQL export unambiguous.
- Import is strict by default: a ragged CSV row or a value that fails the target type aborts before commit. `skip_invalid` is the opt-in lossy mode, and it still reports every skipped row.
- Form PDF fields are single-line and truncate with an ellipsis; multi-paragraph notes are not flowed across pages in v1.0.
- Dropping a table that participates in a relation is blocked until the relation is dropped. Undo restores the table and then the relation; this is deliberate rather than a cascade delete of schema metadata.
- `MIN`/`MAX` rollups that aggregate a computed child column are refreshed lazily for the affected parent instead of maintained in O(1); stored child columns use the full incremental path.
- The audit chain is tamper-evident, not tamper-proof: an attacker with write access to the file can rewrite the whole chain. SQLCipher and external anchoring are the v1.2 hardening step.

## Compatibility policy

Workspace format changes are additive within a major version. A newer build may add `_slab_*` tables or columns, but it will not rename or reinterpret existing user tables. An older build opens a file when the file's `format_version` is less than or equal to the build's supported version. A newer format is rejected rather than opened read-only, because silently hiding metadata leads to incomplete forms.
