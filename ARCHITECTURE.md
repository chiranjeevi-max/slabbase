# Architecture

## Dependency graph

```text
                    apps/desktop (Tauri shell + React UI)
                                  |
        +-------------+-----------+-----------+-------------+
        |             |                       |             |
   slab-storage   slab-import            slab-print     slab-sync
        |             |                       |             |
        +-------------+-----------+-----------+-------------+
                                  |
                             slab-formula
                                  |
                              slab-core
```

Dependencies point downward only. Peers do not depend on one another. `slab-core` has no I/O. `slab-formula` depends on `slab-core` and has no I/O. Storage is the only crate that opens a database. The desktop shell owns all process and thread boundaries.

## Crate responsibilities

### `slab-core`

Pure domain types shared by every layer:

- `Value` with `Null`, `Text`, `Integer`, `Decimal`, `Currency`, `Boolean`, `Date`, `Timestamp`, `Select`, and `AttachmentRef`.
- `Decimal` and `Money`, both backed by `i64` minor units and an explicit scale. Arithmetic uses `i128` intermediates with checked conversion and explicit rounding.
- `Schema`, `Table`, `Column`, `ColumnType`, `Relation`, `IndexSpec`.
- `Layout` and `LayoutItem` for form definitions, including portals.
- `Command` for reversible writes and schema changes.
- `PageQuery`, `Page`, `PortalQuery`, `PortalPage`, `Cursor`, `Filter`, `SortSpec`, and `SearchHit`.
- `ViewConfig` (table, kanban, gallery, calendar) and `PolicyHook` as extension seams.

All wire types derive `serde` and, behind the `ts` feature, `ts-rs`. The Tauri crate enables that feature and exports bindings to `apps/desktop/src/bindings`. Shell-only payloads live in `apps/desktop/src-tauri/src/dto.rs` because import and print crates stay free of IPC concerns.

Relation child columns receive a secondary SQLite index when a relation is created. Relation changes rebuild the child table inside a transaction with foreign keys temporarily disabled, copy every row, rename the rebuilt table, and run `PRAGMA foreign_key_check` before commit. User-created single-column indexes are named with the `slab_idx_` prefix and are preserved across rebuilds.

Table and column renames rewrite metadata, layouts, relations, lookup configurations, and formula source references in one transaction. Dropped tables, columns, layouts, and indexes retain their data in the inverse command so undo restores the exact previous state. Formula and form references block a column drop with a structured error instead of leaving an invalid schema.

### `slab-formula`

A hand-written lexer and recursive-descent parser produce an expression AST. The typechecker assigns one of the core value types and rejects invalid operator or function combinations. The evaluator walks the AST against an `EvalContext`, which supplies column values and aggregate results. The dependency graph orders formula columns and rejects cycles with a deterministic, sorted cycle path.

Formula text is user data. It is never evaluated as code, never shelled out, and never allowed to recurse without a depth limit.

### `slab-storage`

Owns a single `rusqlite::Connection` and the undo journal. Every connection applies:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

Writes follow one path:

1. Begin an immediate transaction.
2. Validate against the schema and formula DAG.
3. Apply the mutation to a plain SQLite table.
4. Serialize the before and after images as JSON.
5. Append the delta to `_slab_audit_log` with an HLC timestamp.
6. Update the FTS index.
7. Commit.
8. Push the inverse command onto the in-memory undo stack.

Undo applies the inverse through the same path, so an undo is itself audited and can be redone.

### `slab-import`

`ImportAdapter` streams rows from a reader. `infer` samples the stream and proposes a schema; `preview` validates every row against the target schema without writing; `apply` hands valid rows to an `ImportSink`. The crate does not know about SQLite, so the same adapters can feed a CLI or a test double.

The CSV adapter strips a UTF-8 BOM before header mapping and reports ragged rows as row issues; `apply` treats those issues like conversion failures, so strict imports never silently drop or reinterpret a row. The JSON adapter distinguishes a clean end of stream from a value truncated at EOF by inspecting the final non-whitespace byte, and missing optional keys are nulls rather than row errors. `Workspace::begin_import`, `insert_import_rows`, and `commit_import` wrap the whole production import in one immediate SQLite transaction, so a failure at any batch rolls back every earlier row. FTS inserts use an append-only path; the former delete-then-insert on every new row made bulk import quadratic in the index size.

### `slab-print`

Consumes a `Layout`, a record, and portal data from `slab-core` and produces either a self-contained HTML document or a PDF byte stream. The PDF writer emits PDF 1.7 with base-14 Helvetica fonts and vector rules, so no font files are redistributed.

PDF text uses WinAnsiEncoding. Latin-1 text and the CP1252 punctuation block (euro, em dash, smart quotes, ellipsis) encode directly. Common symbols outside WinAnsi use deterministic ASCII fallbacks (`₹` to `INR`, `₽` to `RUB`, `−` to `-`, zero-width characters to nothing); other unsupported scripts and emoji become `?`. The fallback is deliberate and stable across exports because embedding a full Unicode font is outside the v1.0 redistribution model. Form fields are single-line and long tokens are truncated with an ellipsis; the table report streams rows and stops at the explicit 25,000-row ceiling.

### `slab-sync`

Defines `HybridLogicalClock`, `ChangeSet`, and `SyncTransport`. v1.0 ships an in-process transport and relies on the same HLC timestamps already written to the audit log. No network code is present.

### `apps/desktop`

The Tauri process owns a worker thread that exclusively owns the `Workspace`. Commands send typed jobs over a channel and await a reply; no SQLite call runs on the UI thread. The React front end holds view state only. Every data operation crosses a typed IPC command, and its TypeScript types are generated from Rust.

## Storage schema

System tables, all namespaced:

| Table | Purpose |
| --- | --- |
| `_slab_meta` | Format version, workspace name, creation time |
| `_slab_tables` | Table identity and physical name |
| `_slab_columns` | Column definitions, types, constraints, formula and rollup config |
| `_slab_relations` | Parent-to-child foreign key relations |
| `_slab_layouts` | Form layouts, stored as JSON validated by `slab-core` |
| `_slab_audit_log` | Append-only before/after deltas with HLC timestamps |
| `_slab_fts` | FTS5 index over text values, keyed by table and row id |

User tables are created with quoted identifiers and an `id INTEGER PRIMARY KEY AUTOINCREMENT` column. Foreign keys are real SQLite constraints with `ON DELETE CASCADE`, `SET NULL`, or `RESTRICT`. Decimal and currency values are stored as `INTEGER` minor units; the scale and currency live in the column definition. Dates are ISO-8601 text; timestamps are Unix milliseconds.

Formula and lookup columns are virtual: they have metadata rows but no physical column. They are computed when a record is read, in dependency order.

## Pagination

`PageQuery` carries a limit (capped at 500), an optional sort, filters, and an opaque cursor. The cursor holds the last row's sort value and primary key. Identifier policy is intentionally narrower than SQLite: names start with an ASCII letter, use ASCII letters, digits, spaces, and underscores, and are at most 64 bytes. Reserved SQLite keywords are ordinary names because every generated statement quotes identifiers. Punctuation, quotes, semicolons, control characters, Unicode, and leading/trailing whitespace are rejected by the typed `SLAB_E_NAME` path before any SQL runs, so hostile input cannot reach DDL. Table, column, and relation renames rewrite formula references by token span rather than substring replacement, preserving string literals and the user's exact formatting through undo.

The generated predicate is:

```sql
WHERE (sort_col > :value) OR (sort_col = :value AND id > :id)
ORDER BY sort_col, id
LIMIT :limit + 1
```

The extra row determines `next_cursor` without a count query. `NULL` values sort last in ascending order. No query materializes a full table.

Child portals use the same keyset path through `PortalQuery` and `PortalPage`, filtered by the parent foreign key. The storage layer returns an exact child count; the desktop portal grid loads pages as the user scrolls and keeps only the loaded rows in memory.

The 100,000-row pagination benchmark runs in release mode against an indexed sort column. The 15 ms p95 assertion is a measurement of indexed keyset pagination, not a promise that an unindexed arbitrary sort will meet the same bound. Users can add a single-column index from the schema management screen.

## Formula DAG

Nodes are `(table, column)` pairs for formula columns. Edges point from a referenced column to the formula that uses it. Before a formula is saved, the proposed graph is topologically sorted. A failure returns `SLAB_E_CYCLE` with the cycle path in stable lexical order, and the transaction is not started.

Aggregates read child rows through relation definitions at evaluation time. Cross-table edges are intentionally excluded from the cycle graph because child aggregates cannot reference the parent formula; this keeps invoice rollups cheap and cycle-free.

## Failure containment

Workspace open rejects empty files, non-SQLite bytes, non-SlabBase SQLite databases, and read-only files before mutating connection settings. The zero-byte and foreign-file cases leave the file byte-for-byte intact. WAL connections set a five-second busy timeout before discovery, so a transient external writer delays the open instead of failing it. Backups use `VACUUM INTO` and reject an existing target.

Full-text search wraps user input in an FTS5 phrase and strips NUL bytes, so operators, unbalanced quotes, punctuation-only terms, and very long strings return results or an empty set instead of a raw SQLite syntax error.

## Non-goals for v1.0

- Multi-user concurrency on one database file beyond SQLite's own locking.
- Network sync, accounts, or hosted storage.
- A scripting runtime or arbitrary code execution in formulas.
- Windows and Linux packaging in the v1.0 verification matrix.
- Rich text, file attachments stored outside `AttachmentRef` paths.
