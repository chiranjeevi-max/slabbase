# Roadmap

## v1.0 — shipped

- SQLite WAL workspace, open format, integrity-checked.
- Schema builder with typed columns and 1:N relations.
- Formula language with acyclic dependency enforcement.
- Virtualized grid, keyset pagination, FTS5 search.
- Form layouts with embedded portals and live rollups.
- Transactional audit log with undo and redo.
- CSV and JSON import with preview; CSV, JSON, and SQL export.
- HTML report preview and vector PDF export.
- Atomic `VACUUM INTO` backups.
- macOS desktop shell, light and dark studio themes.

## v1.1

- Arbitrary column data-type conversion migrations. v1.0 supports renaming, adding, and dropping columns, but changing an existing stored type is deliberately deferred because it requires a row-level conversion plan and rollback proof.
- Additional view types through `slab_core::ViewConfig`: kanban, gallery, calendar.
- Airtable CSV and FileMaker XML adapters implementing `slab_import::ImportAdapter`.
- Saved filters and per-table view presets.
- Multi-column sort and group-by in the grid.
- Batch edit for selected rows.

## v1.2

- Label and barcode templates in `slab-print`, driven by the same layout model.
- Field-level redaction policies through `slab_core::PolicyHook`.
- Encrypted attachment vault hooks, with keys held in the macOS keychain.
- Workspace-level access log export.

## v2.0

- Encrypted LAN, iCloud Drive, or S3 sync implementing `slab_sync::SyncTransport` over HLC change sets.
- Conflict review UI backed by the existing audit deltas.
- Optional native SwiftUI shell through UniFFI over the same crates.
- Windows and Linux installers in the verification matrix.

## Known v1.0 limitations

- PDF text is base-14 Helvetica with WinAnsiEncoding. Latin-1 and CP1252 punctuation are exact; common currency and math symbols transliterate to ASCII; other scripts and emoji render as `?`. Shipping a Unicode font is deferred.
- User identifiers are ASCII (`A-Z`, `a-z`, `0-9`, space, `_`) up to 64 bytes. Non-Latin table and column names are rejected rather than quoted, which keeps formula source and SQL export unambiguous.
- Import is strict by default: a ragged CSV row or a value that fails the target type aborts before commit. `skip_invalid` is the opt-in lossy mode, and it still reports every skipped row.
- Form PDF fields are single-line and truncate with an ellipsis; multi-paragraph notes are not flowed across pages in v1.0.
- Dropping a table that participates in a relation is blocked until the relation is dropped. Undo restores the table and then the relation; this is deliberate rather than a cascade delete of schema metadata.

## Compatibility policy

Workspace format changes are additive within a major version. A newer build may add `_slab_*` tables or columns, but it will not rename or reinterpret existing user tables. An older build opens a file when the file's `format_version` is less than or equal to the build's supported version. A newer format is rejected rather than opened read-only, because silently hiding metadata leads to incomplete forms.
