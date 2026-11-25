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

Dependencies point downward only. Peers do not depend on one another. `slab-core` has no I/O. `slab-formula` depends on `slab-core` and has no I/O. Storage is the only crate that opens a database. The desktop shell owns all process, thread, and concurrency boundaries.

## Crate responsibilities

### `slab-core`

Pure domain types shared by every layer:

- `Value` with `Null`, `Text`, `Integer`, `Decimal`, `Currency`, `Boolean`, `Date`, `Timestamp`, `Select`, and `AttachmentRef`.
- `Decimal` and `Money`, both backed by `i64` minor units and an explicit scale. Arithmetic uses `i128` intermediates with checked conversion and explicit rounding.
- `Schema`, `Table`, `Column`, `ColumnType`, `Relation`, `IndexSpec`.
- `Layout` and `LayoutItem` for form definitions, including portals.
- `Command` for reversible writes and schema changes.
- `PageQuery`, `Page`, `PortalQuery`, `PortalPage`, `Cursor`, `Filter`, `SortSpec`, and `SearchHit`.
- `PageFrame`, `PortalFrame`, and `RowTuple` for compact positional IPC.
- `IndexHint` for the query-plan advisor and `RecordRevision` plus `FieldDiff` for per-record time travel.
- `ViewConfig` (table, kanban, gallery, calendar) and `PolicyHook` as extension seams.

All wire types derive `serde` and, behind the `ts` feature, `ts-rs`. The Tauri crate enables that feature and exports bindings to `apps/desktop/src/bindings`. Shell-only payloads live in `apps/desktop/src-tauri/src/dto.rs` because import and print crates stay free of IPC concerns.

Relation child columns receive a secondary SQLite index when a relation is created. Relation changes rebuild the child table inside a transaction with foreign keys temporarily disabled, copy every row, rename the rebuilt table, and run `PRAGMA foreign_key_check` before commit. User-created single-column indexes are named with the `slab_idx_` prefix and are preserved across rebuilds.

Table and column renames rewrite metadata, layouts, relations, lookup configurations, and formula source references in one transaction. Dropped tables, columns, layouts, and indexes retain their data in the inverse command so undo restores the exact previous state. Formula and form references block a column drop with a structured error instead of leaving an invalid schema.

### `slab-formula`

A hand-written lexer and recursive-descent parser produce an expression AST. The typechecker assigns one of the core value types and rejects invalid operator or function combinations. The evaluator walks the AST against an `EvalContext`, which supplies column values and aggregate results. The dependency graph orders formula columns and rejects cycles with a deterministic, sorted cycle path.

Formula text is user data. It is never evaluated as code, never shelled out, and never allowed to recurse without a depth limit. Table and column renames rewrite formula references by token span rather than substring replacement, preserving string literals and the user's exact formatting through undo.

### `slab-storage`

Owns one serialized writer connection and a bounded pool of read-only connections.

The writer applies:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -64000;
PRAGMA mmap_size = 268435456;
```

Four internal readers open with `SQLITE_OPEN_READ_ONLY`, `PRAGMA query_only = ON`, the same `cache_size` and `mmap_size`, and prepared-statement reuse. Keyset pagination, portal pages, FTS5 search, `EXPLAIN QUERY PLAN`, and streaming PDF reads are routed through the pool, so a long import never blocks an active read inside one workspace. Schema changes reopen the pool so a newly created index is visible to the advisor and to the next page. An in-memory workspace has no second file to share, so its reads deliberately fall back to the writer. The desktop shell adds a second layer: it leases read-only `Workspace` replicas so independent commands can read concurrently, as described under Reader/writer concurrency.

Every interactive search or filtered page registers the active `rusqlite::InterruptHandle` on a shared `QueryCancel`. The shell calls `cancel_search` out of band, without taking the writer lock; the superseded SQLite statement returns `SQLITE_INTERRUPT`, which maps to the benign `SLAB_E_CANCELLED` code and never reaches an error toast.

Writes follow one path:

1. Begin an immediate transaction.
2. Validate against the schema and formula DAG.
3. Apply the mutation to a plain SQLite table.
4. Maintain the affected parent rollups.
5. Serialize the before and after images as JSON.
6. Append the delta to `_slab_audit_log` with an HLC timestamp and a BLAKE3 chain entry.
7. Update the FTS index.
8. Commit.
9. Push the inverse command onto the in-memory undo stack.

Undo applies the inverse through the same path, so an undo is itself audited and can be redone.

### `slab-import`

`ImportAdapter` streams rows from a reader. `infer` samples the stream and proposes a schema; `preview` validates every row against the target schema without writing; `apply` hands valid rows to an `ImportSink`. The crate does not know about SQLite, so the same adapters can feed a CLI or a test double.

The CSV adapter strips a UTF-8 BOM before header mapping and reports ragged rows as row issues; `apply` treats those issues like conversion failures, so strict imports never silently drop or reinterpret a row. The JSON adapter distinguishes a clean end of stream from a value truncated at EOF by inspecting the final non-whitespace byte, and missing optional keys are nulls rather than row errors. `Workspace::begin_import`, `insert_import_rows`, and `commit_import` wrap the whole production import in one immediate SQLite transaction, so a failure at any batch rolls back every earlier row. FTS inserts use an append-only path; the former delete-then-insert on every new row made bulk import quadratic in the index size.

### `slab-print`

Consumes a `Layout`, a record, and portal data from `slab-core` and produces either a self-contained HTML document or a PDF byte stream. The PDF writer emits PDF 1.7 with base-14 Helvetica fonts and vector rules, so no font files are redistributed.

PDF text uses WinAnsiEncoding. Latin-1 text and the CP1252 punctuation block (euro, em dash, smart quotes, ellipsis) encode directly. Common symbols outside WinAnsi use deterministic ASCII fallbacks (`₹` to `INR`, `₽` to `RUB`, `−` to `-`, zero-width characters to nothing); other unsupported scripts and emoji become `?`. The fallback is deliberate and stable across exports because embedding a full Unicode font is outside the v1.0 redistribution model. Form fields are single-line and long tokens are truncated with an ellipsis; the table report streams rows and stops at the explicit 25,000-row ceiling.

### `slab-sync`

Defines `HybridLogicalClock`, `ChangeSet`, and `SyncTransport`. v1.0 ships an in-process transport and relies on the same HLC timestamps already written to the audit log. No outbound network code is present.

### `apps/desktop`

The Tauri process holds an `Arc<DbHandle>` containing a writer mutex, a bounded pool of four read-only `Workspace` replicas, a lifecycle read/write lock, and the shared `QueryCancel`. Read commands lease a replica and run on `spawn_blocking`; writes serialize on the writer mutex. A read therefore completes from the last committed WAL snapshot while an import or schema write is still active, and an open/close waits for in-flight work through the lifecycle lock. The React front end holds view state only. Every data operation crosses a typed IPC command, and its TypeScript types are generated from Rust.

The macOS shell uses native window vibrancy and the system menu bar. `macOSPrivateApi` and a transparent window are enabled only to attach an `NSVisualEffectView` for Glass via `set_window_surface`; Solid clears the effect. The menu bar exposes workspace, edit, view, window, and integrity actions. Appearance supports System, Light, and Dark themes, seven accents plus a custom hex color, and Compact/Default/Comfortable density. `docs/DESIGN_CONTRACT.md` is the normative visual and interaction contract, enforced by the Playwright audit.

## Reader/writer concurrency

The writer is the only connection that mutates the file. `BEGIN IMMEDIATE` and a five-second busy timeout serialize writers without ever holding a read behind them. WAL permits readers to observe the last committed snapshot while the writer holds its transaction, so a 20,000-row import and sustained pagination/search both complete with zero `SQLITE_BUSY` failures. The release stress runs complete 26 page/search batches during one 20,000-row write transaction with zero busy errors. A shell-level test (`apps/desktop/src-tauri/tests/concurrency.rs`) holds an active import transaction open and proves a page read returns the pre-import snapshot without waiting for the writer.

Clean close drops the shell replicas and the internal readers first, runs `PRAGMA optimize`, then `PRAGMA wal_checkpoint(TRUNCATE)` before the writer connection closes. The last connection close removes the WAL and SHM sidecars, leaving one self-contained `.slab` file with every committed mutation in the main database.

## Incremental rollups

A lookup column (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`) keeps a compact `(count, sum, extreme)` state in `_slab_rollups`, keyed by parent table, parent id, relation, child column, and aggregate. On a child insert, update, delete, undo, or import batch the writer adjusts only the affected parents:

- `COUNT`, `SUM`, `AVG`: checked integer or exact-decimal deltas in O(1). `AVG` keeps the exact `(sum, count)` pair and divides only when read.
- `MIN`, `MAX`: compare against the cached extremum in O(1); when the changed child held the extremum, one indexed `ORDER BY ... LIMIT 1` seek refreshes it in O(log N).

Moving a child to another parent removes it from the old parent and adds it to the new one in the same transaction. Cascade deletes remove each captured child from its parent and delete the deleted rows' own parent state. Formula columns remain virtual: they are evaluated in the Kahn order from `FormulaDag` when the record is read, so an affected parent's formulas use the refreshed rollup inputs without rescanning unrelated tables. Schema changes invalidate the cache once; the next read rebuilds only the parents it touches.

The apples-to-apples release benchmark compares the cached rollup against a cold full-table `SUM` over the same rows. At 10,000 children the cached read is 0.021 ms versus 0.303 ms for the cold scan, and the end-to-end child edit plus parent read is 0.169 ms versus 0.692 ms. At 100,000 children the cached read is 0.018 ms versus 3.701 ms, and the end-to-end edit is 0.199 ms versus 3.654 ms. The incremental path removes the per-child scan, so its cost stays flat as the relation grows.

## Storage schema

System tables, all namespaced:

| Table | Purpose |
| --- | --- |
| `_slab_meta` | Format version, workspace name, creation time, and the `audit_head_hash` / `audit_head_id` chain anchors |
| `_slab_tables` | Table identity and physical name |
| `_slab_columns` | Column definitions, types, constraints, formula and rollup config |
| `_slab_relations` | Parent-to-child foreign key relations |
| `_slab_layouts` | Form layouts, stored as JSON validated by `slab-core` |
| `_slab_audit_log` | Append-only before/after deltas with HLC timestamps and BLAKE3 chain hashes |
| `_slab_rollups` | Incremental parent aggregate state used by lookup columns |
| `_slab_fts` | FTS5 index over text values, keyed by table and row id |

User tables are created with quoted identifiers and an `id INTEGER PRIMARY KEY AUTOINCREMENT` column. Foreign keys are real SQLite constraints with `ON DELETE CASCADE`, `SET NULL`, or `RESTRICT`. Decimal and currency values are stored as `INTEGER` minor units; the scale and currency live in the column definition. Dates are ISO-8601 text; timestamps are Unix milliseconds.

Identifier policy is intentionally narrower than SQLite: names start with an ASCII letter, use ASCII letters, digits, spaces, and underscores, and are at most 64 bytes. Reserved SQLite keywords are ordinary names because every generated statement quotes identifiers. Punctuation, quotes, semicolons, control characters, Unicode, and leading/trailing whitespace are rejected by the typed `SLAB_E_NAME` path before any SQL runs, so hostile input cannot reach DDL.

Formula and lookup columns are virtual: they have metadata rows but no physical column. They are computed when a record is read, in dependency order.

## Pagination

`PageQuery` carries a limit (capped at 500), an optional sort, filters, and an opaque cursor. The cursor holds the last row's sort value and primary key.

The generated predicate is:

```sql
WHERE (sort_col > :value) OR (sort_col = :value AND id > :id)
ORDER BY sort_col {ASC|DESC} NULLS LAST, id ASC
LIMIT :limit + 1
```

The extra row determines `next_cursor` without a count query. `NULL` values sort last in both directions. No query materializes a full table.

Child portals use the same keyset path through `PortalQuery` and `PortalPage`, filtered by the parent foreign key. The storage layer returns an exact child count; the desktop portal grid loads pages as the user scrolls and keeps only the loaded rows in memory.

The 100,000-row pagination benchmark runs in release mode against an indexed sort column. The 15 ms p95 budget is a measurement of indexed keyset pagination, not a promise that an unindexed arbitrary sort will meet the same bound. Reference runs land at 1.48 ms median and 2.86 ms p95 over 1,000 pages. Users can add a single-column index from the schema management screen or from the status-bar advisor.

## Index advisor

When a page takes longer than 20 ms, or when an unindexed sort/filter column has at least 1,000 rows, storage runs `EXPLAIN QUERY PLAN` on a reader connection. If the plan reports a `SCAN` of the table and no usable index begins with the active column, the page carries an optional `IndexHint { table, column, suggestedIndex, elapsedMs, scannedRows, detail }`. The shell renders a quiet, dismissible pill in the status bar: `Full scan (38 ms) · Index "Due Date"`. One click calls `create_index`, reopens the reader pool, and reloads the page. The reference measurement on a 100,000-row table moves the first page from 3.02 ms to 0.104 ms after the index is created.

## Tuple-array IPC framing

Table and portal pages cross the Tauri boundary as compact positional frames. Column names travel once per page in `PageFrame.columns` / `PortalFrame.columns`; each row is a `RowTuple { id, values }` whose values align with that array. The storage layer still returns named `Record`s internally, so print, export, and tests keep the readable shape. On a 100-row by 10-column page the JSON payload falls from 35,073 bytes to 23,205 bytes, a 33.8% reduction; wider tables save more because names no longer repeat per row.

## Audit hash chain and record time travel

Each `_slab_audit_log` row carries `prev_hash` and `entry_hash`:

```text
entry_hash = BLAKE3(prev_hash || hlc || table || row_id || diff_json)
```

`diff_json` includes the operation, summary, schema flag, and both before and after JSON strings, so changing any hashed field invalidates the chain. The first entry links to 64 zeroes. `_slab_meta.audit_head_hash` and `_slab_meta.audit_head_id` anchor the tail, which detects a deleted final row. Existing v1.0 files migrate in one transaction: the two columns are added and the chain is backfilled in id order. `Integrity Check` verifies `PRAGMA integrity_check`, `PRAGMA foreign_key_check`, and the full chain, returning an explicit tamper diagnostic naming the first broken entry.

`record_history(table, row_id)` returns the chronological field-level diffs for one record. `restore_record_revision(table, row_id, audit_id)` rebuilds the stored row from that revision, validates foreign keys and the formula DAG, applies the normal update or insert path so rollups stay exact, appends a new `restore` entry to the hash chain, and pushes the inverse onto the undo stack. Restoring is therefore itself undoable with `⌘Z`. The Record Inspector exposes this as a quiet History clock button and an inline revision sheet with before → after values and a `Restore` action.

## Snapshots

`snapshot_to` checkpoints the WAL and, on macOS, attempts an atomic APFS copy-on-write `clonefile` under the writer lock. A cross-device (`EXDEV`), non-APFS (`ENOTSUP`), or non-macOS target transparently falls back to transactional `VACUUM INTO`. Reference runs measure 1.75 ms for a 4.4 MB APFS clone versus 6.44 ms for `VACUUM INTO`. Both paths pass external `sqlite3` integrity and foreign-key checks and preserve the audit chain. Snapshots reject an existing target and never leave WAL or SHM sidecars beside the copy.

## Formula DAG

Nodes are `(table, column)` pairs for formula columns. Edges point from a referenced column to the formula that uses it. Before a formula is saved, the proposed graph is topologically sorted. A failure returns `SLAB_E_CYCLE` with the cycle path in stable lexical order, and the transaction is not started.

Lookup columns read the incrementally maintained `_slab_rollups` state rather than scanning children on every read. Formula-level `aggregate()` calls are evaluated against live child rows on demand and are never cached under a lookup key. Cross-table edges are intentionally excluded from the cycle graph because child aggregates cannot reference the parent formula; this keeps invoice rollups cheap and cycle-free.

## Failure containment

Workspace open rejects empty files, non-SQLite bytes, non-SlabBase SQLite databases, and read-only files before mutating connection settings. The zero-byte and foreign-file cases leave the file byte-for-byte intact. WAL connections set a five-second busy timeout before discovery, so a transient external writer delays the open instead of failing it. Snapshots use the APFS clone path with a `VACUUM INTO` fallback and reject an existing target.

Full-text search wraps user input in an FTS5 phrase and strips NUL bytes, so operators, unbalanced quotes, punctuation-only terms, and very long strings return results or an empty set instead of a raw SQLite syntax error. A superseded search or filtered page is interrupted through `sqlite3_interrupt` and reported as `SLAB_E_CANCELLED`, which the shell treats as a silent no-op.

## Non-goals for v1.0

- Multi-user concurrency on one database file beyond SQLite's own locking.
- Network sync, accounts, or hosted storage.
- A scripting runtime or arbitrary code execution in formulas.
- Windows and Linux packaging in the v1.0 verification matrix.
- Rich text, file attachments stored outside `AttachmentRef` paths.
- Encryption at rest; SQLCipher and Touch ID are scheduled for v1.2.
