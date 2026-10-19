# QZ Flow Roadmap

Approved scope: 19 October 2026, including update prompts, trunk-based development, and GitHub Actions/releases. Milestone 0 infrastructure is implemented; application feature milestones have not started.

## Goal

Make QZ Flow a convenient daily mouse utility for Rexus QZ30 Black on Windows and macOS: run quietly, apply assignments when saved, and provide useful productivity actions with app-specific profiles.

## Approved behavior

- Optional **Launch at login** setting; when enabled, start minimized to the Windows tray or Mac menu bar.
- Clicking the window's **Close** button hides it and keeps mappings running. Normal **Minimize** retains the OS's standard behavior.
- Tray/menu-bar commands: **Open**, **Pause/Resume mappings**, and **Quit**. Quit stops mappings and exits the process.
- **Save assignments** saves and immediately applies the selected mappings. Remove the current enable switch; keep explicit Pause/Resume controls.
- Restore saved mappings automatically on startup, after checking permissions, device availability, and conflicts with the original utility. If blocked, show the reason and leave mappings inactive.
- Failed saves or activation must preserve the previous working assignments and persisted settings. Report partial failures clearly if rollback cannot complete.
- Show a clear **Active**, **Paused**, or **Unavailable** status, with the blocking reason where applicable.
- Offer distinct **Default behavior** and **Disable button** assignments. Do not offer Disable for inputs whose native behavior cannot be reliably suppressed.
- Check for published stable updates automatically and offer an update prompt with version, release notes, **Update**, and **Later**. Download/install only after the user chooses Update; provide manual **Check for updates**.

## Milestone 0 — Trunk-based Git and release foundation

Status: implemented. [PR #1](https://github.com/cunilab/Rexus-QZ-30-mouse/pull/1) passed required Windows/Mac checks and merged through the protected trunk. [Beta release verification](https://github.com/cunilab/Rexus-QZ-30-mouse/actions/runs/39235833219) records exact-tag checks and packaging results. A draft is created only when its platform checks and artifact gates pass; manual publication and installed-app validation remain separate release gates.

Development workflow:

- [x] Use `main` as the single integration trunk, with short-lived `feat/`, `fix/`, and `chore/` branches and pull requests back to `main`.
- [x] Keep changes small, squash-merge completed PRs, and delete merged branches. Do not add long-lived develop or release branches.
- [x] Configure required CI checks and protection against accidental force pushes/deletion on `main`, subject to repository capabilities.
- [x] Use semantic versions and release notes. Keep package.json/package-lock.json, Cargo.toml/Cargo.lock, and tauri.conf.json versions consistent through one release command/check.
- [x] Create immutable `vX.Y.Z` tags from tested commits on `main`; prerelease tags such as `v0.2.0-beta.1` remain outside the stable update feed.

GitHub Actions:

- [x] Extend existing `.github/workflows/check.yml` for PRs and `main`: formatting, UI/Rust tests, Clippy, version consistency, and Windows/Mac build checks.
- [x] Add `.github/workflows/release.yml` triggered by version tags, with manual dispatch for a specified validated tag.
- [x] Reject mismatched tag/application versions and release commits outside `main`; run required checks against the exact release commit.
- [x] Build Windows x64 and separate macOS Intel/Apple Silicon artifacts: NSIS on Windows, app archives and DMGs on Mac. Output must pass signature and inventory verification before draft creation.
- [x] Use official Tauri release tooling to produce installers, updater artifacts, signatures, and updater metadata (`latest.json`).
- [x] Store signing secrets in GitHub Actions secrets; keep the updater public key in app configuration. Updater signatures are separate from Windows code signing and Apple signing/notarization.
- [x] Keep PR workflows read-only and without signing secrets; grant release write permissions only to the release jobs that require them.
- [x] Implement complete-artifact validation and draft-release assembly with changelog, signatures, and checksums. Draft creation results are recorded in the linked release run.
- [x] Preserve a failed build as a failed/unpublished release; never advertise a partial release through the stable updater endpoint.

Verified setup: squash-only merges and merged-branch deletion; required Windows/Mac checks and PRs on `main`; immutable `v*` tag rules; GitHub published-release immutability; updater signing secret configured. Local backup is outside Git. Windows Authenticode and Apple signing/notarization are unavailable and disclosed in release notes. The first pipeline test is `0.1.1-beta.1`; no stable release is published automatically.

Acceptance:

- PRs receive Windows and Mac CI results; a failed required check prevents merging once repository protection is configured.
- A test prerelease demonstrates tag-to-version validation and complete artifact generation without notifying stable users.
- A release cannot overwrite an existing version or silently replace already published artifacts.
- Signing/packaging failures prevent publication; no private keys enter Git, artifacts, or logs.
- Mac signing/notarization and Windows signing availability are recorded before distribution; unsigned builds are labelled accurately.
- GitHub release creation/publishing, repository protections, and CI execution are marked complete only after verified on GitHub, not merely after writing YAML.

Release path: short-lived branch → PR checks → merge to `main` → version/release-note PR → tested version tag → build/sign → draft release → artifact verification → publish.

## Milestone 1 — Background operation and lifecycle

Status: planned.

- [ ] Add tray/menu-bar integration with Open, Pause/Resume, and Quit.
- [ ] Hide on Close; retain normal Minimize behavior.
- [ ] Add Launch at login, initially off; start hidden when launched at login.
- [ ] Prevent duplicate running instances; reopening the app reveals the existing window.
- [ ] Reflect current mapping status in the tray/menu-bar controls.

Acceptance:

- Closing the window leaves verified mappings working; Open restores the window.
- Pause passes original input through; Resume rechecks prerequisites.
- Quit stops the listener and releases any synthetic held keys.
- Enabling/disabling Launch at login takes effect after the next login on each OS.
- Manual launch opens the window; login launch stays hidden unless a permission prompt requires user interaction.
- A second launch never starts a second input listener.

## Milestone 2 — Save and apply

Status: planned; depends on Milestone 1.

- [ ] Replace the enable switch workflow with Save assignments and immediate activation.
- [ ] Permit editing while existing assignments remain active; draft changes take effect only after Save.
- [ ] Validate assignments and prerequisites before replacing working settings.
- [ ] Coordinate persistence and runtime activation, with rollback on failure.
- [ ] Restore saved assignments on startup when prerequisites are satisfied.
- [ ] Add status and actionable errors for missing receivers, permissions, or vendor-utility conflicts.
- [ ] Keep existing user settings compatible with the new format.

Acceptance:

- One Save activates the saved assignments without another switch or button.
- Saving while paused resumes mappings after validation; the UI states this behavior.
- Invalid settings, write errors, and activation errors preserve the previous working state wherever recoverable.
- Upgrade preserves existing assignments; startup never falsely reports Active.
- Device loss stops remapping safely. Reconnection can restore an automatically suspended configuration after prerequisite checks, but must respect an explicit user Pause.
- Default-only settings pass through normally without requiring a non-default action.

## Milestone 3 — Expanded actions and button editor

Status: planned; depends on Milestone 2.

- [ ] Group dropdown choices by purpose.
- [ ] Add custom keyboard shortcut recording, with clear modifier labels for each OS.
- [ ] Add separate Default behavior and Disable button choices where supported.
- [ ] Make the existing QZ30 diagram selectable; highlight detected inputs.
- [ ] Add application launch actions through a native application picker.

Action catalog:

| Group | Actions |
| --- | --- |
| Desktop and windows | Previous/next desktop, Task View/Mission Control, show desktop, switch apps, minimize, maximize/restore |
| Browser and tabs | Previous/next tab, new tab, close tab, reopen closed tab, refresh, back, forward |
| Editing | Copy, paste, cut, undo, redo, select all |
| Media | Volume up/down, mute, play/pause, previous/next track |
| Other | Zoom in/out/reset, screenshot, launch application, custom keyboard shortcut |

Acceptance:

- Every exposed action has an implementation and an accurate label for its platform; unsupported actions are unavailable with a reason.
- One physical press produces one intended action; no stuck modifiers after errors, Pause, or Quit.
- Window actions target the intended foreground window. Platform differences, including Mac maximize versus full screen, are explicit.
- Shortcut recording does not execute the captured shortcut in another app.
- Default preserves native input; Disable suppresses it only where supported.
- OS-configurable shortcuts and app-dependent actions are documented and checked against the test machine's settings.

## Milestone 4 — Side-wheel modes and profiles

Status: planned; depends on Milestone 3.

- [ ] Add Default and per-application profiles, selected automatically by the foreground app.
- [ ] Show the active profile prominently; allow editing a profile without activating its target app.
- [ ] Add side-wheel modes: horizontal scroll, volume, zoom, and tab switching.
- [ ] Add wheel direction and repeat-rate controls.
- [ ] Provide browser, office, and coding starter presets.
- [ ] Add validated local profile import/export, preserving OS-specific shortcut differences.
- [ ] Organize the interface into Buttons, Scroll, and Profiles, with the original QZ30 image and adjacent editor.

Acceptance:

- Switching apps selects the correct profile, with Default as fallback.
- Opening QZ Flow to edit settings does not accidentally redirect an action intended for another app.
- Rapid wheel rotation behaves predictably without duplicate actions or excessive queued shortcuts.
- Imports reject malformed or unsupported data without overwriting working settings.
- Profile export/import round trips preserve supported settings; missing apps and OS differences are reported.

## Milestone 5 — Gestures and shortcut ring

Status: planned; depends on stable input handling and profiles.

- [ ] Add thumb-button tap and hold-plus-movement gestures for up/down/left/right.
- [ ] Add configurable movement thresholds and cancellation.
- [ ] Add a configurable 4–8-action ring near the pointer, triggered by an assigned button.
- [ ] Dismiss the ring with Escape or an outside click; return actions to the original target app.

Acceptance:

- Tap and gesture recognition do not fire together for one interaction.
- Small accidental movements do not trigger a gesture; thresholds are calibrated on real hardware.
- Pause, disconnect, or Quit cancels an unfinished gesture safely.
- The ring remains usable across monitor boundaries and display scaling settings.
- Mac thumb gestures remain unavailable until the physical input path is verified; another supported trigger may be chosen explicitly.

## Milestone 6 — Action sequences

Status: later, after the core features above are stable.

- [ ] Support ordered keyboard shortcuts, text, application launch, and delays.
- [ ] Provide cancellation and stop execution safely when prerequisites fail.
- [ ] Exclude arbitrary shell/script execution from this scope.

Acceptance:

- Sequence ordering and cancellation are verified; no stuck keys remain.
- Focus and timing limitations are visible to users; sequences do not promise reliable application automation where shortcuts cannot provide it.

## Milestone 7 — In-app update suggestions

Status: planned; requires Milestone 0's signed release artifacts and verified update feed, plus Milestone 1's safe shutdown behavior. Can ship before advanced controls once those dependencies pass.

- [ ] Integrate the official Tauri updater with the repository's published stable GitHub Releases feed.
- [ ] Check asynchronously after startup, then at most once daily while running; offer an automatic-check preference and manual Check for updates.
- [ ] Show a prompt for a newer compatible stable version, including version, plain-text release notes, Update, and Later. Later suppresses repeated automatic prompts for that version; a manual check can show it again.
- [ ] If running hidden, show one system notification or tray/menu-bar update indicator; open the prompt on interaction without stealing focus.
- [ ] Show download progress and actionable errors. Verify artifact signatures before installation and ask before the restart required to finish an update.
- [ ] Stop listeners and release synthetic keys before installation/restart. Preserve saved assignments, profiles, and startup preferences; respect an explicit user Pause across an updater restart.
- [ ] Keep offline/rate-limit/server failures from interrupting mouse use; present explicit errors for manual checks.
- [ ] Keep beta/prerelease builds out of the stable feed and select the correct OS/architecture artifact. Do not silently downgrade.

Acceptance:

- A controlled older-to-newer installation verifies check, prompt, download, signature verification, installation/restart, version change, and settings preservation on Windows and Mac.
- Same/older versions do not prompt; Later works; manual checks report current/updatable/unavailable clearly.
- Invalid signatures and incompatible/missing artifacts are rejected before installation.
- Interrupted downloads and unavailable networking leave current mappings usable; failed installation provides a clear error and documented manual recovery path.
- Hidden startup does not repeatedly display windows or update notifications.
- Portable Windows distribution is tested separately; if in-place updates are unsupported, present a clearly labelled manual download flow rather than claiming automatic installation.

## Platform verification and release gates

- Windows receiver side-wheel volume and thumb-button Task View have previously passed user checks. Recheck these after input-engine changes.
- Mac source exists, but build and physical-device validation remain outstanding. Proprietary thumb/side-wheel input support is incomplete; Windows evidence does not establish Mac parity.
- Standard event hooks can affect other mice and trackpads. Test with a second pointing device and disclose any global behavior; do not claim QZ30-only remapping without proof.
- Receiver presence does not prove the mouse is powered on. Keep connection status accurate.
- Validate permissions, original-utility conflict handling, login launch, Pause/Resume, disconnect/reconnect, and shutdown separately on each OS.
- Run focused engine/UI tests and platform build checks for each implementation milestone. User hardware checks are required for completion claims.
- Keep profile/settings upgrades compatible with existing assignments and test recovery from failed writes.
- No account or cloud service is required for the approved features.
- Update checks/downloads require network access to GitHub; core mouse controls remain local and usable offline.

## Deferred boundaries

- Actual sensor DPI, onboard firmware/profile writes, and firmware rebuilding remain unresolved and outside this software roadmap.
- Motorized wheel-mode control, haptic feedback, and automatic device-channel switching require unverified hardware/protocol support.
- Battery percentage is deferred until a reliable readable source is verified for each transport.
- Cross-computer cursor, clipboard, and file sharing would be a separate network feature project.
- Host-side pointer sensitivity is optional future work and must never be presented as sensor DPI.

## References

- [MX-inspired software assessment](analysis/MX-SOFTWARE-PROPOSAL.md)
- [Firmware and DPI research](analysis/FIRMWARE-RESEARCH.md)
- [Logitech MX Master 3S software features](https://support.logi.com/hc/en-za/articles/5218056588311-Getting-Started-MX-Master-3S)
- [Logitech Actions Ring](https://support.logi.com/hc/en-gb/articles/17844647906967-Actions-Ring)
- [Logitech Smart Actions](https://support.logi.com/hc/en-001/articles/14307858722327-How-to-create-a-new-Smart-Actions-on-Logi-Options)
- [Tauri updater and signature requirements](https://v2.tauri.app/plugin/updater/)
- [Tauri GitHub Actions release pipeline](https://v2.tauri.app/distribute/pipelines/github/)
