# Changelog

Release notes use semantic versions. Entries under Unreleased are not a published release.

## [Unreleased]

### Added

- Button 4 (thumb rest) on macOS and Linux and over Bluetooth LE, in addition to Windows with the USB receiver. The OEM handshake and worker are one shared module (`platform/thumb.rs`); macOS uses IOKit HID feature and input reports, Linux uses hidraw (`70-qz-flow.rules` now grants access to the vendor node of both IDs), and Windows also accepts the Bluetooth `ffbb:1` collection. A Bluetooth idle feature report that is all zero no longer blocks the handshake. Status shows Preparing on every OS until the mouse acknowledges. The exchange was measured over Bluetooth on macOS; the Bluetooth press/release frames are assumed to match the receiver's, and macOS, Linux and Bluetooth behavior still needs a physical press. Disable is not offered for button 4. Connect only one of the receiver and the Bluetooth mouse while using button 4.
- Bluetooth LE recognition on Windows, macOS and Linux (`32c2:0012`, "Rexus QZ30"), alongside the USB receiver. Back, Forward, Middle and horizontal scroll use ordinary OS events; Windows receiver side-wheel frames stay USB-only. The Linux udev rule now also covers `32c2:0012`. Windows and Linux Bluetooth behavior is unverified on hardware.
- Linux support (x86_64): the receiver's evdev pointer device is grabbed and re-emitted through a `uinput` virtual mouse, so Back, Forward, Middle and the side wheel can be remapped or disabled on X11 and Wayland. Shortcuts and media keys use a virtual keyboard. A udev rule (`packaging/linux/70-qz-flow.rules`) grants access; see the README.
- Linux AppImage and `.deb` release builds with signed updater artifacts, and a Linux job in the desktop checks.
- Linux input hardening: the listener status is cleared even if the reader thread panics, a failed key release recreates the virtual keyboard so no modifier stays held, side-wheel tilt fires once per notch on high-resolution devices, and launched programs do not inherit AppImage environment variables.
- Linux limits: no shortcut recording, and Show desktop, Minimize and Maximize are rejected because GNOME and KDE share no shortcut for them. Linux hardware behavior is unverified.

### Fixed

- A launch target that is missing, moved or updated no longer disables every mapping. Saving and startup check only that the path is absolute and has the right type; existence is checked when choosing the application and when the button is pressed.
- On Windows, applications start through `ShellExecuteW`, so apps whose manifest requires elevation show the UAC prompt instead of failing with error 740.

### Changed

- Removed the unused virtual-key table for Windows custom shortcuts; they were already sent by physical scan code.

## [0.1.1-beta.6]

### Fixed

- Initialize the QZ30 receiver before enabling button 4's separate input reports, so its saved action can replace the built-in Show Desktop shortcut.
- Perform receiver setup outside engine/UI locks, validate completed acknowledgements, and show Preparing until button 4 is ready. Setup failures pause mappings with an error.
- Restore the OEM default input mask on Pause, Default behavior and normal Quit; prepare the mode again after a detected receiver reconnect or mode reset.
- Wait for Quit cleanup on a background task to keep the UI responsive.

### Validation limits

- A receiver-only physical test confirmed Show Desktop stops and thumb press/release reports arrive after initialization. Integrated beta.6 action execution and forced-exit/reconnect behavior require separate verification.

## [0.1.1-beta.5]

### Fixed

- Release shared engine state before native dispatch so a slow input call cannot hold UI/status access hostage.
- Prevent reentrant injection and stale suppression when a release arrives during dispatch.
- Limit side-wheel desktop switching to one action per 250 ms.
- Track accepted synthetic key events, check immediate partial-injection cleanup, and pause mappings on injection failures.
- Delayed recovery never forces key-ups: uncertain pending keys require manual release before new injection.
- Poll native foreground state during recording without waiting for Tauri's UI thread.

### Validation limits

- Beta.4 caused a reported hang and transient keyboard interference. These repairs address code hazards; the exact incident cause and repaired physical behavior remain unverified. The repaired app has not been started with live mappings.

## [0.1.1-beta.4]

### Added

- Grouped action catalog: desktop switching, window controls, browser tabs, editing, media, zoom, and screenshot shortcuts.
- Custom keyboard shortcuts with modifier/key selection and native focused-window recording.
- Native application picker and application launch assignments.
- Selectable QZ30 mouse markers and last-input highlighting.
- Disable assignments for inputs whose native OS events can be suppressed.

### Changed

- Version 2 settings preserve legacy/version 1 assignments and make an exact backup before migration.
- Application launch runs outside the input listener; asynchronous launch failures appear in status.

### Validation limits

- Physical Windows/Mac actions, native recording/dialog behavior, and OS shortcut configuration need user verification. Secure system shortcuts cannot be guaranteed recordable. Mac proprietary thumb input remains unavailable.

- Dedicated Settings page for Launch at login and window behavior, separate from mouse assignments.

## [0.1.1-beta.3]

### Changed

- Save and apply replaces the enable switch; assignments can be edited while current mappings run.
- Saved assignments restore automatically after receiver, permission, and original-utility checks, including after reconnection.
- Explicit Pause persists across restarts; Save resumes mappings. Default-only assignments work without a receiver.
- Settings migrate without losing legacy assignments; original legacy or invalid files are backed up before replacement.
- Failed activation or persistence preserves previous assignments and reports the failure. A failed Pause write still stops mappings for the current session.

### Validation limits

- Windows physical reconnect/login/tray interaction and macOS runtime require manual checks. Automatic recovery checks prerequisites every two seconds.

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
