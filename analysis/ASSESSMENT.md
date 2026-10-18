# Rexus replacement application: initial assessment

Inspected on Windows device `Andres`, 2026-10-18. This is a static feasibility assessment, not a working replacement or verified hardware protocol.

User confirmed the target is **Rexus Flow QZ30 Black**. Rexus's official [driver page](https://rexus.id/pages/driver) explicitly lists Windows and Mac QZ Flow Series software for QZ5, QZ10 and QZ30 Black only (checked 2026-10-18). This establishes advertised model coverage, not tested compatibility of a new implementation. No QZ30/Rexus marketing name occurs in the extracted model JSON files; the exact internal OEM model ID remains unresolved. Target operating systems remain to be confirmed.

Follow-up: the user selected **both Windows and macOS** and authorized implementation. The first desktop slice is now described in [README](../README.md) and [validation notes](../docs/VALIDATION.md); statements below describe the initial pre-build assessment.

## Result

A new desktop interface is feasible. The supplied packages expose enough model and action information to start designing device selection, button mapping, shortcuts, scrolling, and settings. Hardware compatibility remains the main engineering uncertainty. First prove detection and one reversible remapping for the user's exact model before implementing the full interface.

## Verified files and evidence

| Item | Finding | Evidence |
| --- | --- | --- |
| Windows archive | Contains the Mouse Assistant installer, version 0.0.2.13.20260613 | REA archive inspection `ev_03324a1970a892d084480e02ec0608dfc43a9b240bf884ee982030073ef5803b`; independent ZIP directory read |
| Windows installer | NSIS package containing `MAssistant.exe`, `AMLIB.dll`, and uninstall/plugin files | Static 7-Zip inventory/extraction; installer PE resources `ev_a0422663e1868934f8d511d17cb72c4dfc481b17454ace8fc243aa250460cf90` |
| Windows application | 32-bit x86 native executable with MFC resource markers and an AMLIB import; no recovered application source | REA PE resources `ev_4aa57ab68cb0f8e932f7d6f8197ea6deea65048936b9cdc1eba7b01c84bb054a`; file bytes/import strings |
| Hardware library | 101 named C++ exports, including device discovery, trigger/button operations, DPI, macros, and device-specific command families | Static PE export-table parsing; `windows/amlib-exports.json` |
| Hardware APIs | AMLIB imports HID feature/input/output-report APIs, raw input, Windows hooks, and SendInput | DLL import-name strings; these establish referenced APIs, not observed runtime behavior |
| Windows resources | Embedded ZIP inventories contain 104 model JSON entries, skins and action assets. Non-directory contents in these ZIPs are encrypted. Language JSON is readable and contains 10 language entries | Exact resource ranges from REA, ZIP central directories, `windows/resources/*/inventory.json`, `windows/resources/136/payload.bin` |
| Mac package | `Mouse Setting.app`, identifier `loshine.mouseassistant.dhsoft`, version 1.1/build 20260522; readable model and skin JSON, images, language data | Read-only 7-Zip extraction and Python plist/JSON parsing; REA plist operation is unavailable on Windows |
| Mac models | 94 readable model configurations, with button coordinates, model IDs, masks, USB IDs and RF/BT names | `model-summary.json`; extracted `macos/Mouse Setting/Mouse Setting.app/Contents/Resources/model` |
| Attached hardware | Present Windows mouse/HID interfaces report `VID_276D&PID_1902`; this pair occurs in Mac model configurations | Read-only Get-PnpDevice enumeration, without opening the HID device or sending reports |

The shared USB ID does not uniquely identify the marketed Rexus model. Some configuration IDs may describe receiver/transport matching; exact discovery rules are not yet decoded. Package identity suggests a shared OEM software base, not proof of manufacturer attribution or identical Windows/Mac behavior.

## What can be built

| Capability | Feasibility / remaining proof |
| --- | --- |
| Modern interface | High: original assets need not be reused. A mouse diagram, clear button assignments, and visible connection state are practical design choices. |
| Standard button/shortcut remapping | Practical first implementation target. Determine which events the exact mouse exposes, then test suppression and replacement without duplicate clicks. Extra controls may require vendor reports. |
| Scroll/tilt actions, navigation, media controls | Existing labels and library API references provide leads. Actual event delivery and model support need verification. |
| DPI and report interval | Exports exist, but device support, parameter units, readback, and persistence are unverified. Do not promise universal support. |
| Mac version | Resources help explain model/UI behavior. Runtime and hardware testing must happen on macOS; Windows cannot establish Mac compatibility. |
| Profiles or per-app mappings | Possible new product features; not established as original behavior and unnecessary for the first hardware proof. |

Action asset names also suggest screenshot, magnifier, spotlight, gesture, and productivity features. Filenames alone do not establish implementation or availability. Gaming and firmware exports may belong to other supported hardware families.

## Implementation choices

1. **Windows prototype using the original DLL:** investigate the exported ABI and the app's call sites, then use a small matching 32-bit native helper if viable. This may preserve compatibility sooner, but C++ structs, callbacks, MFC strings, library initialization effects, and redistribution terms are unresolved. It is not a drop-in generic FFI library.
2. **Independent device implementation:** recover only the selected model's HID commands and OS input behavior, then build the new UI around that implementation. Better long-term ownership and a possible cross-platform path, with more initial protocol and hardware testing.

Recommended sequence: identify the marketed model and desired operating systems; prove read-only device identification and understand the relevant event/report path; test one reversible remap; then build the smallest useful interface. Choose the UI stack after deciding the hardware integration route. Deep native call-site analysis needs an analysis engine such as Ghidra, which is not configured here.

## Boundaries and retained artifacts

- The supplied installers, app, and DLL were not executed or loaded. No driver was installed, no firmware/EEPROM writes were sent, and mouse settings were not changed.
- No new app has been built, and the live interface has not been visually inspected. Findings about features are static observations or explicitly marked leads.
- Original packages remain intact. Extracted material and inventories are under `analysis/`; REA's five retained evidence records are in `rea-evidence.json`.
- Do not treat the evidence bundle's `unknowns: 0` export count as proof that all investigation questions are answered; the unresolved questions above remain.
- Windows encrypted resource contents were not decrypted. Mac readable resources are sufficient for this initial assessment, but version/platform differences remain.

## Artifact hashes (SHA-256)

```
Windows archive: 25d5c4ff00817a6e17afd5bf8f404a5447e8c7492c040be94199d57eddc13f53
Windows installer: d8de70ef3668e439ce9e64e33718e28d3e20eaa62a7cd06146366dbf72aae7db
MAssistant.exe: 20cb6cc57b3cf79d8f8de331cd238863511b4ca8eedb805bbdb3c2cdbf676074
AMLIB.dll: 9fef9362244f959158a9330f6a3d856000f089a77013de16aa39f043bd2521f3
Mac executable: 4648c5c00217f8245c48703434b911bfbcce83a4a15af49dd388107c0d11a3f2
```
