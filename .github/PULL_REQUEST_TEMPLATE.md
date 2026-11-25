## Summary

Describe the problem, the change, and the affected crate or surface. Link the issue or discussion when one exists.

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo test --release -p slab-storage --test pagination --test systems`
- [ ] `cargo deny check`
- [ ] `npm --prefix apps/desktop run typecheck`
- [ ] `npm --prefix apps/desktop run build`
- [ ] `npm --prefix apps/desktop run test:ui`

Note any check that could not run locally and why.

## Contract and scope

- [ ] Every commit is signed off under the DCO (`git commit -s`), and signed when a key is configured (`git commit -S -s`).
- [ ] Every commit subject is lowercase Conventional Commits and strictly under 55 characters.
- [ ] The change keeps the crate dependency direction in `ARCHITECTURE.md` intact.
- [ ] User-facing visual or interaction changes update `docs/DESIGN_CONTRACT.md` and pass `test:ui`.
- [ ] Storage-format or public-behavior changes update `ARCHITECTURE.md` and `CHANGELOG.md`.
- [ ] No generated artifact, personal workspace, secret, or local override file is included.

## Risk and rollback

Explain the failure modes, the compatibility impact, and how to revert the change if needed.

## AI assistance disclosure (optional)

If substantial AI-agent assistance was used, disclose it here and add a standard `Co-authored-by:` trailer to AI-authored commits, as described in `AGENTS.md`. A local `AGENTS.local.md` override takes precedence for maintainers.
