# Contributing to QZ Flow

Use `main` as the integration trunk. Create short-lived `feat/`, `fix/`, or `chore/` branches from current `main`, open a small pull request, and squash-merge after review and required checks pass. Delete the merged branch. Do not create long-lived develop or release branches or force-push `main`.

## Local checks

Use Node.js 24, the stable Rust toolchain with rustfmt/Clippy, and the platform prerequisites for Tauri 2. Windows and macOS builds must be checked on their respective operating systems.

```sh
npm ci
npm run version:check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
npm test
npm run check
npm run build -- --no-bundle
```

`npm test` includes the UI tests, release/version tooling tests, and Rust tests. Keep platform behavior accurately labelled: a Windows build does not verify Mac mouse input or accessibility permissions.

Before merging, require the platform checks in the repository's `main` protection/ruleset settings and block force pushes and deletion. Workflow files alone do not configure these settings. Repository settings and hosted checks must be verified separately; do not treat their configuration as complete without checking GitHub.

## Versions and releases

Version changes belong in a small pull request with a `CHANGELOG.md` entry. From the repository root:

```sh
npm run version:set -- 0.2.0
npm run version:check -- --tag v0.2.0
```

The command updates all five version files together without making a commit or tag. Use semantic versions with optional prerelease identifiers (`0.2.0-beta.1`). Build metadata (`+build`) is intentionally unsupported. See [Releasing](docs/RELEASING.md) for the tag, draft, signing, and publication procedure.
