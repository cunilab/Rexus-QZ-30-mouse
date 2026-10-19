# Changelog

Release notes use semantic versions. Entries under Unreleased are not a published release.

## [Unreleased]

## [0.1.1-beta.2]

### Added

- Optional launch at login, with login launches hidden in the tray or Mac menu bar.
- Close to tray/menu bar, with Open, Pause/Resume mappings, status, and Quit commands.
- Reopening the app restores the existing window instead of starting another listener.
- Startup paths support spaces and XML characters; Windows registration affects only the current user and rejects paths exceeding its startup-command limit.

### Validation limits

- Windows/Mac native GUI clicks and actual login behavior require manual checks; the Computer Use helper was unavailable during development.
- Automatic mapping activation and save-and-apply remain Milestone 2.

## [0.1.1-beta.1]

Pipeline-validation prerelease; not a stable release or a claim of Mac hardware validation.

### Added

- Trunk-based contribution and release instructions.
- Version consistency checks and an explicit version-setting command.
- Windows/macOS CI and draft-release packaging foundation.
- Signature and complete-artifact validation for Windows x64 and Mac Intel/Apple Silicon updater packages.

### Fixed

- Mac volume-event integer types now match AppKit's native API.

## [0.1.0]

Initial development version; this heading does not imply that a signed release has been published.

- QZ30 button and side-wheel assignment interface.
- Windows input mappings, including verified volume control and thumb-button Task View.
- Mac action implementation; physical proprietary-input support still requires Mac validation.
