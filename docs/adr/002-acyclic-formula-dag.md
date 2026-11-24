# ADR 002: Formula dependencies form an acyclic graph

## Status

Accepted.

## Context

Formula columns are convenient precisely because they can reference other calculated values. That convenience becomes a correctness problem when a user creates `Total = Subtotal + Tax`, then edits `Tax = Total * 0.08`. A spreadsheet detects this at evaluation time and shows an error in a cell; a database schema cannot leave rows in an undefined state.

Formula text is also untrusted input. It must not be able to execute code, reach the network, read files, or exhaust the stack.

## Decision

Before a formula column is saved, its references are collected and inserted into a dependency graph whose nodes are `(table, column)` pairs. The candidate graph is topologically sorted inside the schema-change transaction. If sorting fails, the transaction rolls back and the caller receives `SLAB_E_CYCLE` with a deterministic cycle path.

Aggregates such as `SUM(LineItems.Amount)` read child rows through declared relations. They are evaluated after scalar dependencies and do not create cross-table graph edges, because a child formula cannot reference the parent row.

Formula evaluation is a tree walk over a parsed AST with a fixed recursion limit. There is no dynamic code execution. Unknown functions and type mismatches are rejected at schema time, not at read time.

## Consequences

- Cycles are impossible by construction, not repaired at read time.
- Rejected schema edits leave the workspace untouched.
- Recomputation order is stable and testable.
- Cross-table rollups are cheap but cannot participate in a cycle; a future version that allows parent references from child formulas will need graph nodes that include foreign key paths.
- The parser and DAG are pure functions over `slab-core` types, so they are covered by property tests without a database.
