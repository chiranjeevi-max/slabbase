# Releasing SlabBase

The release workflow builds, tests, signs when credentials are available, attests, and creates a **draft** GitHub Release. It never publishes a release automatically. A maintainer promotes the draft after a smoke test.

## Preconditions

1. `main` is green in CI, CodeQL, and Scorecard.
2. All milestone pull requests and pre-release checks are merged.
3. The version is identical in `Cargo.toml`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.
4. `CHANGELOG.md` has an entry for the release.
5. The signing secrets are configured if this is a signed distribution.

The release workflow enforces the version parity gate and fails before building when the tag, workspace version, package version, and Tauri version disagree.

## Cut a draft release

```bash
git switch main
git pull --ff-only
git tag -s v1.0.0 -m "SlabBase v1.0.0"
git push origin v1.0.0
```

The `v*.*.*` tag triggers `.github/workflows/release.yml`. The workflow:

1. Checks out the tag and validates version parity.
2. Runs `cargo test --workspace`, `cargo deny check`, and the Playwright UI audit.
3. Builds an auditable binary for `universal-apple-darwin` when both Rust targets install, otherwise `aarch64-apple-darwin`.
4. Builds the app and DMG through Tauri with Developer ID signing and notarization when the Apple secrets are present; otherwise builds the app with `--no-sign`, ad-hoc signs it, and creates the DMG with `hdiutil`.
5. Verifies the `.dep-v0` section in the raw and bundled binaries.
6. Produces `SlabBase_<version>_<arch>.dmg`, `sbom.cdx.json`, `SHA256SUMS.txt`, and generated release notes.
7. Creates SLSA build provenance and SBOM attestations with `actions/attest`.
8. Creates a draft GitHub Release with the DMG, SBOM, and checksums attached.

The runner is `macos-15`. Do not use `macos-14`.

## Signing and notarization secrets

Configure these as repository or organization Actions secrets. The workflow builds signed only when both `APPLE_CERTIFICATE` and `APPLE_SIGNING_IDENTITY` are present, and unsigned only when both are absent. If exactly one of those two values is present, the workflow fails fast rather than producing a partially configured build. When signing is requested, all credential values below are required.

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64 of the Developer ID Application `.p12` certificate. |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | Full identity, for example `Developer ID Application: Studio Name (TEAMID)`. |
| `APPLE_ID` | Apple ID used for notarization. |
| `APPLE_PASSWORD` | App-specific password for the Apple ID. |
| `APPLE_TEAM_ID` | Ten-character Apple Developer team identifier. |

Create the base64 value without line wrapping:

```bash
base64 -i DeveloperID.p12 | tr -d '\n' > apple-certificate.txt
```

Tauri imports the certificate and performs notarization and stapling during `tauri bundle`. Keep the `.p12`, its password, and the app-specific password out of the repository and out of workflow logs.

## Verify a downloaded release

```bash
shasum -a 256 -c SHA256SUMS.txt
gh attestation verify SlabBase_1.0.0_universal.dmg --repo chiranjeevi-max/slabbase
gh attestation verify SlabBase_1.0.0_universal.dmg --repo chiranjeevi-max/slabbase --predicate-type https://cyclonedx.org/bom
gh attestation verify SHA256SUMS.txt --repo chiranjeevi-max/slabbase
```

The first command checks the DMG and the SBOM against `SHA256SUMS.txt`. The attestation commands verify the Sigstore bundle against the repository's public attestation store, including the CycloneDX SBOM predicate. Review `sbom.cdx.json` for the dependency inventory.

## Gatekeeper and unsigned builds

A signed and notarized build should open without an override. An unsigned build is still useful for testing and local distribution, but macOS may quarantine it. After copying it to `/Applications`, remove the quarantine attribute:

```bash
xattr -dr com.apple.quarantine /Applications/SlabBase.app
```

The generated draft release notes include this note only when the build is unsigned.

## Promote the draft

Smoke-test the downloaded DMG on a clean macOS account: launch, click **Load Sample Workspace** (or open a local copy of `fixtures/design-studio.slab`), edit a record, inspect the portal, open History, run `Integrity Check`, export a PDF, and quit. Then publish the draft:

```bash
gh release edit v1.0.0 --draft=false --latest
```

## Rollback

If the draft is defective, delete it before promotion:

```bash
gh release delete v1.0.0 --yes
```

If the tag itself is wrong, delete the remote and local tag, fix the release commit, and tag again:

```bash
git push origin :refs/tags/v1.0.0 && git tag -d v1.0.0
```

Never move a published tag; publish a new patch version instead.
