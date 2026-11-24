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
- `PageQuery`, `Page`, `Cursor`, `Filter`, `SortSpec`, and `SearchHit`.
- `ViewConfig` (table, kanban, gallery, calendar) and `PolicyHook` as extension seams.

All wire types derive `serde` and, behind the `ts` feature, `ts-rs`. The Tauri crate enables that feature and exports bindings to `apps/desktop/src/bindings`. Shell-only payloads live in `apps/desktop/src-tauri/src/dto.rs` because import and print crates stay free of IPC concerns.

Relation child columns receive a secondary SQLite index when a relation is created. Relation changes rebuild the child table inside a transaction with foreign keys temporarily disabled, copy every row, rename the rebuilt table, and run `PRAGMA foreign_key_check` before commit.

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

### `slab-print`

Consumes a `Layout`, a record, and portal data from `slab-core` and produces either a self-contained HTML document or a PDF byte stream. The PDF writer emits PDF 1.7 with base-14 Helvetica fonts and vector rules, so no font files are redistributed.

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

`PageQuery` carries a limit (capped at 500), an optional sort, filters, and an opaque cursor. The cursor holds the last row's sort value and primary key. The generated predicate is:

```sql
WHERE (sort_col > :value) OR (sort_col = :value AND id > :id)
ORDER BY sort_col, id
LIMIT :limit + 1
```

The extra row determines `next_cursor` without a count query. `NULL` values sort last in ascending order. No query materializes a full table.

## Formula DAG

Nodes are `(table, column)` pairs for formula columns. Edges point from a referenced column to the formula that uses it. Before a formula is saved, the proposed graph is topologically sorted. A failure returns `SLAB_E_CYCLE` with the cycle path in stable lexical order, and the transaction is not started.

Aggregates read child rows through relation definitions at evaluation time. Cross-table edges are intentionally excluded from the cycle graph because child aggregates cannot reference the parent formula; this keeps invoice rollups cheap and cycle-free.

## Non-goals for v1.0

- Multi-user concurrency on one database file beyond SQLite's own locking.
- Network sync, accounts, or hosted storage.
- A scripting runtime or arbitrary code execution in formulas.
- Windows and Linux packaging in the v1.0 verification matrix.
- Rich text, file attachments stored outside `AttachmentRef` paths.
