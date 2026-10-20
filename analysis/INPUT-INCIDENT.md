# QZ Flow input incident — 2026-10-19

Device: Windows / Andres. Affected preview: 0.1.1-beta.4, SHA-256 `1bfab2af687dd8f9dccf6720bc7519098349cc02bdcf3d39751d449d995e9217`.

## Observations

- The user reported Not responding, keyboard interference, and Escape behaving like the Windows key. They confirmed this happened while using mouse wheel/buttons, without selecting Record shortcut, and that the keyboard returned to normal after closing QZ Flow.
- On inspection QZ Flow was not running. Original Mouse Assistant was running; it was left untouched. The preview has not been reopened.
- Saved version 2 assignments were `wheel_left=previous_desktop`, `wheel_right=next_desktop`, `thumb=switch_apps`, other inputs default, and `paused=false`. Startup reconciliation therefore restores Ctrl+Win+Arrow and Alt+Tab assignments. Settings have not been changed during this investigation.
- PR #6 was converted to a draft. The Settings-page PR #5 merged separately; the new input actions are not merged into main.
- Local unit/build checks passed before the incident. They do not establish native input safety or explain this failure.

## Findings in QZ Flow source

1. `Shared::process` holds the engine mutex while calling native `perform`, including `SendInput`. Status/UI operations need that same mutex. A blocked native call can therefore block the interface as well. This is a verified coupling, not proof of the particular hang.
2. Partial SendInput failure triggers another release call, but its return value is ignored. There is no durable record of unreleased synthetic modifiers. Incomplete modifier release is consistent with the report; no event trace or dump proves it happened. [SendInput documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) describes partial results and keyboard-state interference.
3. Recorder focus checks can wait on Tauri's UI thread before timeout polling. That is a separate safety issue, but recording was not used in this incident, so it is not the leading explanation. [Keyboard hook documentation](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) requires a responsive message loop.
4. No explicit keyboard-layout or pointer-speed writes were found in the input paths reviewed. Recovery after closing the process supports a transient input-state problem; this is not a complete audit of Windows settings.

## Original application / REA comparison

Target: supplied `analysis/windows/AMLIB.dll`, SHA-256 `9fef9362244f959158a9330f6a3d856000f089a77013de16aa39f043bd2521f3`. This is static inspection, without loading the DLL, modifying the original application, or sending device reports.

REA procedure inventory `ev_25fce42bb6acfbd3006f6834fdad61054dd7544f77ac4fa56b43ff8c69ae8981` identifies SendInput, Windows hook APIs, raw-input APIs, AM_EnableRawInput and AM_SetTriggerEvent. Imports alone do not establish runtime dispatch/release behavior or prove the original application is safe. Focused decompilation is complete for seven functions. Compact exact excerpts and Evidence IDs are in [REA input comparison](REA-INPUT-COMPARISON.json).

- `AM_EnableRawInput` only writes a library flag (`ev_db21d7a8ccfb9186a6e3a0cb50393fb856c2c6ffaf76b1cd5b5751f1dc7a71aa`); this is not a magic hardware isolation API.
- The recovered dispatcher `0x10032d60` reads raw input, optionally forwards it to a callback, and calls a mouse-input handler for mouse reports (`ev_5ac1ef7e08fc9e6fa38ddf55a5c8c31b0fb102b6eb4c754e59af9491cd7c75ac`).
- In examined handler `0x10031f10`, device-state lookup releases its mutex before the later keyboard injection branches (`ev_a2fcdf16b8442737d88d0874e0bf9b68d87be07b78610a09aec2a09204aae8c2`). This contrasts with QZ Flow's engine mutex spanning native dispatch; it does not prove the original never holds any lock during input.
- A recovered original key helper sends one key-down and then one key-up, and ignores both SendInput return values (`ev_59cbe677e5335cfeaf962c1d04fe7d355d162f533ac7fbb44fd993053aa4fc48`). Copying that helper would not provide verified failure recovery.
- The examined hook initializer installs a thread-specific CBT hook, type 5 (`ev_a9c46698d8e6c80cb19e3e94cca7b06af0e8ecc507383534fe87369a6c0d143f`). Importing hook APIs alone is not proof of a global keyboard hook. Other branches and the live original application were not exhaustively traced.

Decompiler union field labels are not source types: with INPUT.type=1, the overlapping field at offset 4 carries keyboard flags (0/down or 2/up), while the value 0xb9db51 is the library's extra-info marker. Do not interpret the displayed MOUSEINPUT field names as actual mouse-event flags.

## Required before another physical test

- Keep the current preview closed and this PR unmerged.
- Separate input dispatch from engine/UI locks; bound repeated system-navigation dispatch.
- Verify partial-injection cleanup and track only synthetic keys owned by QZ Flow; avoid changing keys the user physically holds.
- Make recorder cancellation independent of UI responsiveness before exposing it again.
- Start future previews without restoring live mappings. Validate one action at a time on hardware, with explicit recovery checks and useful native error telemetry.

The exact incident cause remains unresolved. Do not present these hypotheses as an observed runtime trace.


## Repair work ? 2026-10-20

Both applications were stopped at the continuation check. The saved QZ Flow mappings were then paused, preserving assignments and keeping an exact sibling backup `mappings.input-incident-ea136c2879e34500b07982c585b43a9d.backup.json` in the Windows app configuration directory. This prevents automatic restoration on a later launch.

Repairs implemented with local fake-input verification: release the engine mutex across native dispatch; reject reentrant injection and avoid stale suppression after an in-flight release; bound side-wheel desktop navigation to one step per 250 ms; account for inserted synthetic keys and check immediate cleanup; require manual release rather than forced delayed key-ups; remove Tauri UI waits during native recorder focus checks. These address confirmed code hazards, not a demonstrated single root cause. No repaired app, input hook, or physical action has been executed during this repair.

Local verification: 32 Node tests, 53 Windows Rust tests, formatting, version consistency and Clippy pass. Independent safety review passed after fixing delayed recovery and reentrant-release handling. Optimized build and hosted platform checks are recorded in PR #6. No claim of physical recovery is made.
