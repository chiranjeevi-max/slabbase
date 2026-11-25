# Security Policy

## Supported versions

| Version | Security fixes |
| --- | --- |
| 1.0.x | Yes |
| < 1.0 | No |

## Reporting a vulnerability

Report privately through GitHub Private Vulnerability Reporting on the `chiranjeevi-max/slabbase` repository. Open the Security tab, choose "Report a vulnerability", and include the version, macOS version, a minimal workspace file when required, and the steps that trigger the behavior. Do not open a public issue for an undisclosed vulnerability.

## Response targets

| Step | Target |
| --- | --- |
| Acknowledge report | 3 business days |
| Initial assessment and severity | 10 business days |
| Fix or documented mitigation for critical issues | 30 days |
| Fix or documented mitigation for high issues | 90 days |
| Public advisory | After a fixed release is available |

Severity follows CVSS 4.0 with the understanding that SlabBase is a local application. A vulnerability that requires an attacker to already have write access to the workspace file is treated as lower severity unless it crosses a privilege boundary.

## Scope

In scope:

- Memory safety in the Rust crates, including the bundled SQLite boundary.
- SQL injection through table names, column names, filters, sort keys, or import headers.
- Formula parser or evaluator denial of service, stack exhaustion, or unintended file or network access.
- Path traversal in import, export, snapshot, print, or attachment handling.
- IPC command authorization in the Tauri shell, including commands that accept arbitrary paths.
- Integrity failures that allow user data to be corrupted or silently lost.

Out of scope:

- Attacks that require the user to run untrusted code with their own privileges.
- macOS platform vulnerabilities.
- Denial of service through a deliberately enormous but well-formed database that the user opened themselves, unless it bypasses the documented pagination limits.
- Reports produced solely by a scanner without a reproduction.

## Security design notes

- Formula text is parsed, never executed. There is no `eval`, shell, or dynamic library loading.
- SQL identifiers from user input are validated against the schema and quoted; values are bound parameters.
- The desktop shell makes zero outbound network calls. The webview loads only bundled assets, and the Playwright air-gap test asserts that booting the app issues no external HTTP or HTTPS requests.
- `macOSPrivateApi` and the transparent window are enabled strictly to attach an `NSVisualEffectView` for native Glass vibrancy through `set_window_surface`. Solid mode clears the effect. No private API is used for filesystem, network, or process access.
- The BLAKE3 audit chain detects edits, deletions, and tail truncation performed outside SlabBase because every entry commits to its predecessor and `_slab_meta` anchors the head. A log with missing hashes is not automatically repaired: only a wholly unhashed legacy log with no chain anchor is backfilled, in one transaction, so NULLing every hash cannot rewrite a fresh chain over altered content. It is tamper-evident, not tamper-proof: an attacker who rewrites the entire chain and head metadata can forge a consistent replacement. SQLCipher encryption and external anchoring are the v1.2 hardening steps.
- CSV export prefixes text that starts with `=`, `+`, `-`, `@`, tab, carriage return, or line feed with an apostrophe. The apostrophe is part of the exported text, so a SlabBase re-import preserves it; it is a display-safety prefix, not a reversible encoding. A spreadsheet treats the cell as text rather than a formula.
- CSV and JSON exports publish atomically from a scratch file in the destination directory. Re-exporting over an existing file keeps that file's permissions, so a `0600` export is not widened to the default mode.
- The Tauri capability set is minimal: core window operations, file dialogs, and opening exported files.
- Import paths, export paths, print paths, and snapshot paths are chosen by the user through native dialogs in the desktop build. The Rust API itself accepts paths because it is also used by tests and embedding hosts.
- `cargo deny check` runs on every release and rejects known advisories, wildcard dependencies, unknown registries, and unapproved licenses.
- SlabBase crates are Apache-2.0 with no copyleft code. The desktop shell links Tauri, whose macOS HTML and CSS handling pulls a small set of file-level MPL-2.0 transitive helpers (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`). These crates are unmodified, are not part of any SlabBase crate, and are named as the only license exceptions in `deny.toml`.
- Release builds are reproducible from a pinned `Cargo.lock` and are `cargo-auditable` compatible.
