# ADR 001: Open SQLite WAL as the workspace format

## Status

Accepted.

## Context

Commercial form-and-database products commonly store workspaces in proprietary binary containers. That choice creates migration costs, prevents inspection with standard tools, and makes an exit from the product expensive. SlabBase targets professionals who keep client, patient, and legal records; they need to know the data survives the application.

The storage engine must support transactions, durable writes, secondary indexes, full-text search, and foreign keys without building each of those from scratch.

## Decision

A `.slab` workspace is a SQLite 3 database in WAL mode. User data lives in ordinary tables with ordinary constraints. SlabBase metadata lives only in tables prefixed `_slab_`.

Every connection sets:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

Decimal and currency values are stored as integer minor units, never as floating point. Dates are ISO-8601 text. Timestamps are Unix milliseconds.

## Consequences

- Users can inspect, query, copy, and back up workspaces without SlabBase.
- SQLite's WAL gives crash safety and concurrent readers without custom code.
- SQLite's type affinity is weaker than a static schema. SlabBase validates values in Rust before writing and generates `CHECK` constraints for booleans and selects.
- WAL leaves `-wal` and `-shm` files beside the database while it is open. Backup uses `VACUUM INTO` so a snapshot never depends on those files.
- Schema changes are constrained by SQLite's `ALTER TABLE`. Destructive changes are staged through table rebuilds in a transaction, not performed in place.
