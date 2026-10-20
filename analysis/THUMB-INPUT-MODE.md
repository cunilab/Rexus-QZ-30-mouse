# Button 4 input-mode investigation

Windows device Andres, 2026-10-20; QZ Flow beta.5 (`8d08cb1`).

## Observed problem

The saved thumb assignment is `volume_up` with `paused: false`, but the user reports Show Desktop. The running executable matched the verified beta.5 SHA256. Mouse Assistant was not running. Therefore this is not explained by an unsaved assignment or the original utility running concurrently.

A receiver-filtered Raw Input capture recorded repeated `VK_LWIN` down, `D` up, `VK_LWIN` up events from `276d:1902`. It did not record the complete Win+D sequence, so do not invent the missing D-down event. The user independently confirmed Show Desktop. No `bb04000400000012` thumb notification arrived; side-wheel vendor frames did arrive. QZ Flow currently handles the thumb vendor notification, not receiver keyboard shortcuts. This explains why its saved action does not execute on the observed input path. It does not identify why the mouse entered this mode.

The capture registered Raw Input without suppressing events and discarded unrelated devices before reading key fields. It did not install a keyboard hook, inject keys, or record the laptop keyboard. Captures and diagnostic scripts are local-only under `.release-test/`.

## OEM evidence

- Existing REA evidence for `FUN_100262d0` and `am_loyal_sendCommand` recovers command2 with a complete 16-bit little-endian input mask at offsets4/5, report ID `bb`, XOR checksum seeded with `12`.
- The original `15_1051/model.json` places Show Desktop at index10 and declares availability mask `0x71c`. This supports candidate thumb mask `0x400`, but is not a verified runtime index-to-mask transformation.
- Read-only HID caps confirm usage `ffbb:1` has feature length8 on this receiver. This was measured separately from input-report length.
- New REA evidence `ev_82fef97a6828be1edd4be24b4e33c6374f361babd2ccef8043afc97632a7b247` decompiles original-app cleanup `FUN_004199c0` at `0x4199c0`: remove tray icon, enumerate devices, `AM_SetButtonMask(uid,0,0,0,type)`, free library. Thus mask0 is an OEM exit default, not an exact getter/restore of the prior mask.
- Existing type17 initialization performs a challenge command0 and command1 before registration. The challenge uses bytes `[bb,00,01,02,03,A,B,checksum]`, with response byte5 `((A+B)&255)^55` and byte6 `((B+response[5])&255)^11`.
- Independent read-only review confirmed these encodings and found that the OEM wrapper's timeout comparison does not match its loop limit. Copying its return behavior would not establish completion.
- Additional static procedures `0x10034ea0` and `0x100357b0` were inspected, but did not establish the missing thumb configuration path; no conclusion relies on them.

## Bounded device test and cleanup

QZ Flow was closed before the test; no action injection ran during it.

1. Initial feature state: `bb00010203b72e8b`.
2. Windows accepted candidate mask packet `bb02000000040014`. Immediate feature state `bb82000000040094` was busy/pending, not a completion acknowledgement.
3. The user reported Show Desktop still occurred. Receiver keyboard events continued; no thumb vendor notification appeared. This negative direct-write test does not disprove the candidate mask because the OEM initialization prerequisite was not established.
4. A `finally` block sent OEM-default reset `bb02000000000010`; Windows accepted it. Immediate state was pending `bb82000000000090`. A later read-only query returned ready state `bb02000000000010`, consistent with that reset. Exact prior-mask restoration and crash cleanup remain unverified.
5. Only after that ready state, a source-derived initialization challenge was sent. The first response failed checksum validation, so the diagnostic stopped before command1 or another mask write. A later read returned `bb000102038445d3`; the required transformed challenge response was not established. No firmware, EEPROM or DPI command was sent.

## Verified initialization and thumb mode

The first initialization diagnostic omitted the OEM's 100ms delay before inspecting the response. Retrying the exact sequence with that delay and bounded polling for valid, non-busy packets succeeded:

- Challenge: `bb00010203d2db1b`; validated response: `bb00010203f8c228`.
- Completed command1 response: `bb0101010f00001c`.
- Completed thumb mask response: `bb02000000040014`.
- The user confirmed **Show Desktop stopped**. The receiver capture recorded repeated thumb press `bb04000400000012` and release `bb04000000000016` pairs. This establishes the input path after initialization, not every action's native execution.
- Completed cleanup response: `bb02000000000010`.

The earlier negative direct-write result was insufficient: initialization is needed before the observed thumb mode works. API success, a busy packet, and a valid completed acknowledgement remain distinct. Selected exact records are retained in `analysis/THUMB-MODE-EVIDENCE.json`. Full local records are `.release-test/thumb-initialize-delayed.jsonl`, `thumb-auth-probe.jsonl`, and `thumb-auth-input.jsonl` on Andres.

## Product repair

Beta.6 implements the verified exchange for one matching Windows `276d:1902`, `ffbb:1` receiver with feature length8. Device requests run on a dedicated worker outside engine/UI locks. A non-default thumb assignment enables the mode; Pause, Default behavior and normal Quit request the checked OEM-default reset. Setup failures pause mappings and expose the error. Status remains Preparing until acknowledgement, and a periodic read-only mode check detects receiver loss or a changed acknowledged mask. Expected disconnection does not change the Pause preference; reconnection prepares the mode again. Quit waits for cleanup on a background task.

Protocol fake-I/O tests cover malformed/busy replies, challenge matching, cancellation and failed cleanup; worker tests cover lock ownership, Pause, shutdown reset, disconnect and mode loss. Integrated app execution is a separate acceptance check.

No global keyboard shortcut remap is added. Firmware, EEPROM and DPI writes remain out of scope. Individual synchronous HID calls have no OS timeout; polling has a finite budget, and the dedicated worker prevents those calls from blocking normal UI/status access. Forced process termination cannot guarantee reset, and exact prior-mask restoration is not claimed.

Earlier Task View success remains historical evidence for another observed state, not proof that cold-start thumb configuration is implemented. The prior keyboard-interference incident's exact trigger also remains unproven.
