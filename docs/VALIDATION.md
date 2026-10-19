# Validation — first desktop slice

## Milestone 3 ? 0.1.1-beta.4

- Local Windows checks: 32 Node tests and 44 Rust tests passed, with version consistency, formatting and Clippy warnings denied. Optimized native build is checked separately from physical input execution.
- Added coverage for structured action validation, version 1 migration/backups, raw-receiver Disable rejection, physical scan-code mapping/extended releases, recorder cancellation/drain, deep draft isolation, native editor cancellation, and mapped-input bypass during recording.
- Independent cross-scope review verified launch paths remain data, application work runs outside input callbacks, and recording guard cleanup preserves consumed releases. Focus and held-key findings were fixed and reviewed.
- Hosted Windows/Mac checks and physical OS configuration are separate gates. Native recording, dialogs, app launches, window actions and non-US layouts still require user verification; secure OS shortcuts cannot be guaranteed captured.


## Milestone 2 ? 0.1.1-beta.3

- Local Windows validation: 24 Node tests, 32 Rust tests, formatting, Clippy with warnings denied, version consistency, and optimized Tauri build passed.
- Tests cover save-before-runtime-commit ordering, failed persistence retaining active assignments, listener loss during persistence, persistent Pause, default assignments without a receiver, strict settings migration/backup, and editable UI drafts.
- Independent read-only review passed after fixing nonprompting Mac permission preflight and unsupported saved-mapping errors.
- Required Windows/Mac PR checks are recorded in [PR #4](https://github.com/cunilab/Rexus-QZ-30-mouse/pull/4). Hosted build success does not establish physical Mac compatibility.
- Updated portable app was opened on Windows device Andres. Original Mouse Assistant remained running; its conflict guard must be respected. No new physical remapping, tray-click, reconnect, actual login, or Mac runtime pass is claimed.

The sections below are historical evidence; their test counts and enable-switch instructions describe earlier versions.


Device: Windows `Andres`, 2026-10-18. Target: user-confirmed QZ30 Black, Windows and macOS.

## Original control layout and input capture update

- Numbering now follows the supplied screenshot: Middle 1, Back 2, Forward 3, thumb-rest 4, plus Side up/down. Task View / Mission Control and volume actions are available. Existing five-key settings load with thumb at default; no saved assignments were overwritten.
- Read-only captures `analysis/button-trace-1.jsonl` and `analysis/button-trace-2.jsonl` observed vendor frames `bb04000000010017`, `bb04000000ff00e9`, and neutral `bb04000000000016`. Windows now listens to the observed vendor collections, filters the matching receiver, and accepts only the two exact non-neutral frames. No device report was sent. Physical action execution is a separate check.
- A later isolated capture (`analysis/thumb-only-trace.jsonl`) correlated thumb-rest presses with `bb04000400000012`, released by neutral `bb04000000000016`. Windows button 4 is now supported with paired state and duplicate press filtering; macOS keeps it disabled. No Windows XBUTTON3 mapping is assumed.
- The user confirmed physical side-wheel volume changes after QZ Flow reported active mappings. Original Mouse Assistant was closed. Thumb Task View execution is tracked separately below.
- The user then confirmed button 4 opens Windows Task View. Native REA evidence `ev_19380ccb2611e0d9b8fec4fbb667dc1e028881636fab8eb2bd9975b94eb65095` shows the saved volume assignments, thumb Task View assignment, and active listener before that check. No page errors were observed in that attachment window. The test app's temporary CDP endpoint was removed by closing that process and reopening the normal executable.
- Five Node tests and thirteen Rust tests pass; Clippy with warnings denied and the optimized Windows build pass. New checks cover exact frame acceptance/rejection, thumb edges, old settings migration, numbering, action options and platform thumb availability.
- Final browser visual capture `ev_3cf0b06d33f6042e8c9161e6a3c435b4e0c66f65ace87d2d7687e3dc5bc430fd` shows the updated UI without observed page errors. `docs/preview.png` is this browser preview.
- macOS adds Control+Up for Mission Control and native AppKit media-key events for volume. These changes have not been compiled or executed on a Mac. Its private side-wheel reports are not handled; the existing horizontal OS-event path remains.

The remaining sections record earlier checks and their limitations.

Visual update: replaced the generic SVG with the original app's `15_1051.png`, copied unchanged. The shell/wheel layout visually matches Rexus's official QZ30 Black photo; this does not verify its internal device protocol ID. UI marker positions use the original model JSON. Four interface tests and the release build pass after the change. REA inspected the updated browser-rendered interface with zero observed page errors (`ev_21aba0a84a7dec7ae6ea98f6423205cd74940f5c4abafea241101e7cae7d3c4f`). The current `docs/preview.png` is that browser preview, not a new native/hardware test. Attribution is in `ui/assets/NOTICE.md`.

## Verified

- Final Windows checks: nine Rust tests and four Node tests passed; Clippy passed with warnings denied; the optimized Tauri Windows build succeeded. The portable executable is approximately 8.7 MB before compression. These checks do not establish Mac compatibility or physical remapping.

- Windows native build runs and identifies the observed `276d:1902` compatible receiver via Win32 raw-input inventory.
- `--check-hook` reports a running listener with `enabled: false`, then exits successfully and tears it down. No shortcut or hardware report was sent by that check.
- Rust tests cover disabled/default pass-through, paired release suppression after disabling, duplicate-down handling, failed injection pass-through, input validation and atomic repeated settings saves.
- Four Node tests cover browser preview restrictions, preserving unsaved drafts during polling, serialized save/read behavior, save-before-enable rules, and error recovery.
- REA exercised the actual Windows WebView2 application, not just a browser mock. The UI reported the real receiver, saved Back → Copy, and refused enable because the original Mouse Assistant was running. The test reset and saved all mappings to defaults afterward. No physical button remap was executed.
- REA rendered the interface and captured `docs/preview.png`; no page errors were observed during these captures. Polling state and controls remained functional. Browser-preview and native captures are distinct.

Native REA evidence: initial application `ev_fb459425d478357a66e11b2ee10b4e1f4fc250f7385e07cbc65258d2b0c8ebb1`, save/conflict flow `ev_c02cc2bfc08b9e3889f09a80fc0d398ff99a1b4c806b2875c1b76fce4a60ea8e`, restored defaults `ev_c6e8315e8c25a9509528c1516beaa15556ebac50bf57987b1bba46e95026d82d`. REA's event completeness is limited to the attachment window; zero observed page errors does not establish complete runtime coverage.

## Still requires testing

- Actual QZ30 side-button and thumb-wheel events with Mouse Assistant closed, and a reversible remap in a disposable document. The original utility was left running and untouched.
- macOS native build, permissions, HID registry values, button numbering, shortcuts and event-tap lifecycle on real hardware. Cross-compilation from this Windows host failed in a dependency needing an Apple native compiler. The CI definition is present but has not run remotely.
- Bluetooth discovery and model-specific HID/vendor commands are not implemented. DPI/report-rate/macros/firmware controls remain outside this first slice.
- The OS-level listener is global. Elevated Windows applications can reject synthetic shortcuts. Natural scrolling, non-US keyboard layouts and simultaneous multiple-mouse button presses need additional hardware validation.

## Review and limits

An independent read-only review identified duplicate-down pass-through and macOS tap/shutdown issues. Those findings were fixed with a regression check for duplicate presses, a bounded run-loop shutdown, and hook-state recovery. The independent review did not complete because the child agent hit its usage limit. Subsequent checks and fixes were performed by the main agent; they are not an independent review pass.

Desktop automation helpers were unavailable, so REA inspected and exercised the application's own WebView2 test endpoint. That debugging endpoint belongs only to the temporary debug process; it is not configured in the shipped app.
