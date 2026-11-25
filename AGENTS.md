# AGENTS.md

If an untracked AGENTS.local.md exists at the repository root, read it alongside this file for local environment and workflow preferences.

## Project and architecture map

SlabBase is a native macOS relational database, form studio, and report workbench. A workspace is a standard SQLite 3 database in WAL mode; metadata lives in `_slab_*` tables and user data lives in ordinary tables with real foreign keys.

Dependency direction is strictly downward:

```text
slab-core -> slab-formula -> slab-storage / slab-import / slab-print / slab-sync -> apps/desktop
```

`slab-core` is pure domain types. `slab-formula` is the lexer, parser, typechecker, evaluator, and dependency DAG. `slab-storage` owns every SQLite connection, DDL operation, pagination path, rollup, audit entry, and undo command. `slab-import`, `slab-print`, and `slab-sync` are independent peers. `apps/desktop` is the Tauri shell, concurrent read pool, typed IPC boundary, and React UI.

Repository layout:

- `.github/` — CI, security, nightly, release, and community templates.
- `apps/desktop/` — React front end, Playwright audit, Tauri shell, generated TS bindings.
- `crates/` — the Rust workspace; dependency direction follows the line above.
- `docs/` — design contract, ADRs, release procedure, screenshots.
- `fixtures/` — generated sample workspace; treat it as read-only test input.

Read these before changing behavior:

- `ARCHITECTURE.md` — normative crate graph, storage schema, concurrency, pagination, rollups, audit chain, failure containment.
- `docs/DESIGN_CONTRACT.md` — normative visual and interaction contract, enforced by `apps/desktop/tests/ui-audit.spec.ts`.
- `docs/adr/` — accepted architecture decisions.
- `SECURITY.md` — threat model, supported versions, and private reporting channel.
- `ROADMAP.md` — shipped scope, next work, known limits, compatibility policy.

Storage is the only crate allowed to open SQLite. `slab-core` and `slab-formula` must not gain filesystem, network, SQLite, UI, or runtime dependencies. Do not hand-edit `apps/desktop/src/bindings/`; regenerate with `cargo test -p slab-core --features ts`.

## Non-negotiable engineering invariants

- **Purity at the bottom.** `slab-core` and `slab-formula` remain 100% pure: no I/O, SQLite, network, clock, process, or UI dependencies. Keep the workspace dependency direction acyclic and downward.
- **No panics in library code.** Every library crate carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`. Return typed errors with stable `SLAB_E_*` codes, and keep user input on the typed validation path before SQL or DDL runs.
- **Exact arithmetic.** `Money` and `Decimal` use integer minor units, an explicit scale, and checked `i128` intermediates. Never introduce binary floating point into currency, totals, tax, or rollup arithmetic.
- **Open format.** Preserve the SQLite WAL contract: `_slab_*` metadata tables, real foreign keys, incremental `_slab_rollups`, BLAKE3-linked audit entries with `audit_head_hash` anchors, and the reader/writer split in `DbHandle`. A clean close checkpoints and truncates the WAL; a crash must still leave a valid database.
- **IPC contract parity.** Every frontend `invoke` in `apps/desktop/src/api.ts` must match a handler in `generate_handler!`. `every_frontend_invoke_has_a_registered_handler` in `apps/desktop/src-tauri/tests/contract.rs` must pass. Update the generated bindings whenever a wire type changes.
- **UI contract.** Literal colors live exclusively in `apps/desktop/src/styles/tokens.css`. Data grids and form sheets remain opaque (`alpha >= 0.96`) in Glass mode. Virtualizers stay synced to the active row density. Development fixtures must never leak into the production `dist/` bundle.
- **Bounded memory.** Grids and portals use keyset pagination and virtualized rows. No read path may materialize a full table.
- **Audit is atomic.** A mutation, rollup adjustment, FTS update, before/after delta, BLAKE3 chain entry, and inverse command commit together. Undo and restore are audited writes, not shortcuts around the normal path.
- **Cycles fail before mutation.** A schema change that would introduce a formula cycle returns `SLAB_E_CYCLE` before any transaction starts.

## Canonical build, test, and verification commands

Run from the repository root. Do not substitute a narrower command and call the gate complete.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --release -p slab-storage --test pagination
cargo test --release -p slab-storage --test systems
cargo run --release -p slab-storage --example systems_bench
cargo deny check
cargo deny check licenses
```

```bash
npm --prefix apps/desktop ci
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:ui
```

```bash
cargo install cargo-auditable
cargo auditable build --release -p slabbase-desktop
otool -l target/release/slabbase-desktop | grep dep-v0
```

```bash
npm --prefix apps/desktop run tauri build
```

The release binary and bundle are `target/release/slabbase-desktop` and `target/release/bundle/macos/SlabBase.app`. Regenerate bindings with `cargo test -p slab-core --features ts` after changing `slab-core` wire types.

## Change boundaries

- **Never mutate `fixtures/design-studio.slab` in place.** Copy it to a temporary directory before a test that writes. The tracked fixture must stay byte-identical.
- **Do not widen `apps/desktop/src-tauri/capabilities/`.** Do not add network access. `test:ui` enforces zero outbound HTTP(S) requests at boot.
- **Do not alter `LICENSE`, `NOTICE`, or the license allowlist in `deny.toml`** without explicit maintainer approval. New dependencies must pass `cargo deny check` and the existing license policy.
- **Keep versions locked.** `Cargo.toml`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json` share one version. The release workflow rejects a tag that does not match all three.
- **Do not change the workspace format casually.** Any metadata or SQLite schema change needs an additive migration, a compatibility test, and an update to `ARCHITECTURE.md` and `ROADMAP.md`.
- **Do not bypass the UI contract.** Token changes belong in `tokens.css`; interaction changes belong in the design contract and the Playwright audit, not in ad hoc CSS overrides.
- **Do not commit generated artifacts.** `target/`, `node_modules/`, `dist/`, Playwright reports, and local workspaces stay untracked.

## Testing rules

- A bug fix needs a regression test that fails before the fix.
- New behavior needs a focused unit or integration test; storage behavior also needs the release pagination/systems coverage.
- Formula changes must keep the property tests, adversarial tests, and cycle ordering green.
- Storage changes must keep the external `sqlite3` integrity, foreign-key, audit-chain, and undo parity checks green.
- UI changes must pass `test:ui` at the contract viewports, including reduced motion, high contrast, Glass opacity, and air-gapped boot.
- Tests must not depend on the network, a real user workspace, or a machine-specific path. Use temporary directories and explicit fixtures.
- If a check cannot run locally, state that in the pull request and leave CI as the authority.

## Definition of done

A change is ready when all of the following are true:

- The smallest behavior-specific test fails before the change and passes after it.
- The full canonical gate above is green, or the pull request names the check that could not run and why.
- Public behavior, storage format, or visual contract changes are reflected in the matching normative document.
- `git diff --check` is clean and no generated artifact or local workspace is staged.
- The commit is signed off, the subject is under 55 characters, and the pull request explains the risk and rollback path.

## Commit, pull request, and AI contribution policy

- Use lowercase Conventional Commits, imperative mood, and a subject strictly under 55 characters. One logical change per commit.
- Every commit requires a DCO sign-off: `git commit -s`. Sign commits cryptographically when keys are configured: `git commit -S -s`.
- Do not add tool attribution, generated-by trailers, or unrelated co-author trailers to commits, code, comments, or documentation. The AI-assistance disclosure below is the only exception.
- Human accountability is mandatory: review, understand, and locally verify every changed line before opening a pull request. Do not submit code you cannot explain or maintain.
- **AI assistance disclosure:** pull requests with substantial AI-agent assistance must disclose that assistance in the PR description and include a standard `Co-authored-by:` trailer on AI-authored commits.
- Do not commit secrets, tokens, personal data, or a personal `.slab` workspace. Report vulnerabilities privately under `SECURITY.md`.
- Pull requests must include the verification commands that were run, note any skipped check, and update `CHANGELOG.md` for user-visible changes.
- Keep documentation and comments factual. Explain why, invariants, and safety properties; do not restate the next line of code.

## Escalation

When a change crosses a non-negotiable invariant, stop and ask the maintainer rather than widening scope. When repository instructions conflict, follow this order: this file, `ARCHITECTURE.md`, `docs/DESIGN_CONTRACT.md`, `SECURITY.md`, `CONTRIBUTING.md`.
