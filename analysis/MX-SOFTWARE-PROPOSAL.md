# MX-inspired QZ Flow proposal

Review proposal, 2026-10-19. No application code or settings changed.

Goal: make QZ Flow a useful productivity utility for the QZ30 on Windows and macOS, using host-side software features. This is a feasibility assessment, not a compatibility test or implementation commitment.

## Current baseline

Reviewed ui/main.js, ui/index.html, src-tauri/src/engine.rs, src-tauri/src/main.rs and both platform implementations.

- One global mapping set, six configurable inputs, twelve non-default actions.
- Windows receiver side wheel and thumb button have previously passed user checks.
- No application profiles, custom shortcut recorder, gesture recognizer, radial overlay, or macro engine.
- macOS thumb input is disabled; proprietary side-wheel reception is not implemented. Standard OS horizontal scroll handling exists but has not been validated on Mac.
- Standard event remapping can affect other pointing devices. Device isolation needs explicit validation before claiming QZ30-only behavior.

## Recommended first release

| Feature | Proposed behavior | Feasibility and boundary |
| --- | --- | --- |
| Interactive mouse diagram | Click a numbered QZ30 button to configure it; highlight received input | Extend current original-device image and existing last-input reporting |
| Custom keyboard shortcuts | Record a combination; add media, window and application-launch actions | Host-side implementation on both OSes, with permissions and shortcut differences |
| Application profiles | Default profile plus automatically selected per-app assignments; visible active profile | New foreground-app detection and profile storage on both OSes |
| Side-wheel modes | Horizontal scroll, volume, zoom, tab switching; direction and repeat-rate controls | Windows receiver input already verified; shortcut behavior varies by target app; Mac input validation required |
| Import/export and presets | Local profile files; browser, office and coding starter profiles | Software-only; validate imports and keep OS-specific shortcuts explicit |

Suggested interface: Buttons, Scroll, Profiles; profile selector at top, mouse image in center, selected control editor beside it. Use QZ Flow's own styling and existing QZ30 image.

## Second release candidates

- Thumb gestures: tap retains an action; hold plus movement selects up/down/left/right actions. Windows needs movement capture, thresholds, cancellation and pointer behavior testing. Mac thumb input remains a prerequisite. Try middle-button fallback only if chosen by the user.
- Quick ring: configurable 4–8-action overlay near the pointer, triggered by a mapped button; dismiss with Escape or click outside. Preserve the original foreground app when executing shortcuts. No haptics promised.
- Action sequences: ordered shortcuts, text, app launch and delays, with cancellation. Keep shell/script execution out of the initial scope. Timing and focus make sequences less reliable than single shortcuts.

## Defer or exclude

- Actual sensor DPI, onboard profile writes and firmware updates: no verified QZ30 command or recovery route.
- Motor-controlled wheel mode changes, SmartShift-equivalent mechanics and haptics: no verified QZ30 software interface/hardware support.
- Battery indicator: show only after a real readable source is verified per transport; never estimate a percentage.
- Automatic switching between computers and clipboard/file sharing: a separate network input-sharing project. QZ30 channel-switch commands are unknown; do not promise Logitech Flow compatibility.
- Pointer sensitivity: separate optional host-side feature, never label it sensor DPI; not necessary for the first release.

## Verification gates

- Profile switches follow foreground applications and fall back to Default.
- Existing mappings survive upgrade, and failed saves preserve previous settings.
- Each mapped press produces exactly one intended action; modifiers are released after cancellation/errors.
- Side-wheel behavior is checked in the actual supported apps, including rapid rotation.
- Windows receiver and Mac hardware checks are reported separately; no untested parity claims.
- Confirm scope with a second mouse/trackpad; disclose global behavior where device isolation is unavailable.

## Primary references

- Logitech MX Master 3S setup: https://support.logi.com/hc/en-za/articles/5218056588311-Getting-Started-MX-Master-3S
- Logitech Actions Ring: https://support.logi.com/hc/en-gb/articles/17844647906967-Actions-Ring
- Logitech Smart Actions: https://support.logi.com/hc/en-001/articles/14307858722327-How-to-create-a-new-Smart-Actions-on-Logi-Options

These documents establish Logitech behavior. QZ30 feasibility above is our engineering assessment from the local source and previous device evidence; Logitech software/protocol support is not assumed.
