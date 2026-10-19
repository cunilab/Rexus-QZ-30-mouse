# Releasing QZ Flow

Releases come from tested commits on `main`, using immutable tags such as `v0.2.0` or `v0.2.0-beta.1`. A tag starts packaging; it is not approval to publish. Release automation creates drafts, and a maintainer publishes only after verifying the complete release.

## Prepare the version

1. Create a short-lived release-preparation branch from current `main`.
2. Run `npm run version:set -- 0.2.0` and add release notes under `## [0.2.0]` in `CHANGELOG.md`. Use the actual intended version.
3. Run the checks in [CONTRIBUTING.md](../CONTRIBUTING.md), review the diff, and open a pull request. Squash-merge after the Windows and Mac checks pass.
4. Fetch `main` and tags, choose the tested commit, and run `npm run version:check -- --tag v0.2.0` from that checkout. Tag that exact commit and push the tag. Never move or reuse a release tag.

Version tooling reads `package.json`, both root version fields in `package-lock.json`, the `qz-flow` package in `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`. `check` fails on missing, malformed, or inconsistent versions, or a tag that does not exactly equal `v` plus the application version. `set` accepts an explicit strictly increasing semantic version, including prereleases, but excludes build metadata. It preserves dependency versions, writes each file through a temporary file and rename, and attempts to restore originals if a write fails. Filesystem failure during rollback is reported and requires manual repair; interruption or power loss is not a transaction across files. It never commits, tags, pushes, installs dependencies, or publishes.

## Signing prerequisites

The Tauri updater requires its own signing key pair. `TAURI_SIGNING_PRIVATE_KEY` is the GitHub Actions secret used by the release workflow; the matching public key is in `src-tauri/tauri.release.conf.json`. The configured key has no password. Its local backup is outside this repository at `%USERPROFILE%\.codex\signing\qz-flow\updater.key`; keep a secure backup because future updates must remain verifiable by installed apps. Updater signatures authenticate update packages and are separate from operating-system signing.

- Windows Authenticode requires an appropriate code-signing certificate or supported signing service.
- macOS distribution signing and notarization require Apple credentials and a Developer ID identity/certificate.
- Windows Authenticode and Apple signing/notarization are not configured. The draft must disclose this: Windows may warn about an unknown publisher, and macOS may block opening an unsigned/non-notarized app. Updater signatures do not remove these OS checks.
- Do not assume a signing credential works because a workflow references it. Verify signing results before distribution, and update this document when OS signing is configured.
- Do not commit private keys or include them in artifacts/logs. Do not publish updater metadata whose signing key differs from the key trusted by the app.

Signing availability and the first hosted packaging run remain external release gates. The updater client and user-facing update prompt are a later roadmap milestone, not a consequence of adding release infrastructure.

## Build, inspect, and publish

`.github/workflows/release.yml` runs for pushed `v*` tags and accepts a manual `tag` input for an existing version tag. It validates tag/application agreement and that the commit belongs to `main`, then runs reusable checks against that exact commit. The pinned official Tauri action builds a Windows x64 NSIS installer and separate macOS Intel (`x86_64`) and Apple Silicon (`aarch64`) app/DMG packages, plus signed updater artifacts, using `src-tauri/tauri.release.conf.json`.

Packaging output is staged and validated across all three targets before checksums and `latest.json` are assembled and a draft is created. An existing GitHub Release is rejected rather than having its assets overwritten. Use the workflow's configured signing prerequisites; never bypass a failed signing/build gate to publish partial output. The first hosted run must verify this path end to end; local configuration is not proof that GitHub produced or signed working packages.

Inspect the draft before publishing:

- Version, tag commit, changelog, prerelease flag, and platform/architecture labels are correct.
- All intended installers, updater packages, signatures, and checksums are present and match the downloaded files.
- Updater metadata references the exact release/version, includes every supported platform, and uses valid signed artifacts.
- Install and launch on each platform, verify settings survive upgrades, and verify the OS-signing status claimed in the release notes.
- A test prerelease proves the complete process without entering the stable update feed. An end-to-end update test is required once the updater client is implemented.

Only then publish the draft manually. Keep failed or incomplete releases unpublished. Prereleases stay marked as prereleases and outside stable updates. Never overwrite a published version's assets or replace its tag; fix mistakes with a new version. Existing published release protection and manual publication must be checked on GitHub, not inferred from local YAML.
