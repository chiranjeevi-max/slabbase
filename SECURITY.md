# Security Policy

## Supported versions

| Version | Security fixes |
| --- | --- |
| 1.0.x | Yes |
| < 1.0 | No |

## Reporting a vulnerability

Report privately through GitHub Security Advisories on the `slabbase/slabbase` repository, or by email to `security@slabbase.app`. Do not open a public issue.

Include the version, macOS version, a minimal workspace file if one is required to reproduce, and the steps that trigger the behavior. Encrypted archives are acceptable; send the passphrase through a separate channel.

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
- Path traversal in import, export, backup, print, or attachment handling.
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
- The Tauri capability set is minimal: core window operations, file dialogs, and opening exported files.
- The desktop shell does not enable remote content. The webview loads only bundled assets.
- Import paths, backup paths, and export paths are chosen by the user through native dialogs in the desktop build. The Rust API itself accepts paths because it is also used by tests and the CLI.
- `cargo deny check` runs on every release and rejects known advisories, wildcard dependencies, unknown registries, and unapproved licenses.
- SlabBase crates are Apache-2.0 with no copyleft code. The desktop shell links Tauri, whose macOS HTML and CSS handling pulls a small set of file-level MPL-2.0 transitive helpers (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`). These crates are unmodified, are not part of any SlabBase crate, and are named as the only license exceptions in `deny.toml`.
- Release builds are reproducible from a pinned `Cargo.lock` and are `cargo-auditable` compatible.
