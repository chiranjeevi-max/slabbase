# Fixtures

`design-studio.slab` is a generated sample workspace. It is a standard SQLite 3
database in WAL mode and can be inspected without SlabBase:

```bash
sqlite3 fixtures/design-studio.slab '.tables'
sqlite3 fixtures/design-studio.slab 'PRAGMA integrity_check;'
sqlite3 fixtures/design-studio.slab 'SELECT Number, Total FROM Invoices;'
```

Regenerate it from source:

```bash
cargo run -p slab-storage --example seed_fixture
```

The sample contains four tables (Clients, Projects, Invoices, Line Items), three
relations, formula columns for line amounts and invoice rollups, and three form
layouts. The generator clears the undo stack before closing; the audit table
still records how the sample was created.
