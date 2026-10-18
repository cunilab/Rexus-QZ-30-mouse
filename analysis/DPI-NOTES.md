# QZ30 DPI investigation

Windows device Andres, 2026-10-18. Requested behavior: configure DPI levels, rather than assign the button a shortcut.

## Established

- The user identified the small button behind the main scroll wheel. Rexus's product image labels that position **DPI**, with levels **1000 / 1600 / 2400 / 3000 / 4800**. This establishes the advertised button and levels, not an API for changing them.
- Primary source: https://rexus.id/products/rexus-flow-qz30-mouse-office-wireless-ergonomic-silent-click-aerospin-wheel-horizontal-scroll-mode . Diagram: https://rexus.id/cdn/shop/files/MP_Mouse_QZ30_PUTIH-15.jpg . Reference downloaded locally under `analysis/`.
- The user reported no pointer speed change during the first physical check. `dpi-button-trace.jsonl` contains mouse/wheel input and neutral receiver frames, but no isolated DPI notification. Absence from this capture does not prove a defective button or absence of an internal DPI change.
- The supplied original-app screenshot has button action assignments only. The readable `15_1051/model.json` has no DPI configuration fields; this bounded search does not prove that every original-app feature was inspected.
- QZ Flow currently sends no HID output reports and has no DPI read/write implementation. No app setting or sensor DPI was changed during this investigation.

## Static library leads, not a usable QZ30 API

The original DLL exports generic DPI functions. Read-only PE parsing/disassembly using isolated analysis dependencies (`pefile` and `capstone` under `analysis/python-tools/`) found:

- `AM_GamingSaveDPI` at analysis VA `0x1000e1e0` gates its write paths on internal types 3, 4 and 5; other values reach the return path with initial return value `0x100`. This has not been correlated with the QZ30's live internal device record. The asset model type 17 is not assumed to be the same field.
- `AM_SetTempDPI` at `0x10012e60` dispatches through an optional callback in the device record; this does not prove a callback exists for this receiver or define DPI units/persistence.
- The AIT and PAR2801 implementations construct different proprietary messages. Neither is established as applicable to the QZ30, so neither was sent.
- REA's native providers are unavailable here: Ghidra is not configured, Hopper is unsupported on Windows, and IDA is not configured. Static fallback disassembly is not runtime evidence and does not replace model-specific command verification.

## Next check

Both QZ Flow and Mouse Assistant were closed for a second physical check, to separate the reported unchanged pointer speed from utility behavior. Await that result, then reopen the normal QZ Flow executable. Custom DPI-level editing remains unimplemented until the selected model's supported command, units, bounds, readback and persistence are established. Do not substitute Windows pointer speed for sensor DPI.
