# QZ30 firmware research

Checked 2026-10-19 from Windows device Andres. Public web research only; no device writes or flashing.

## Verified public information

- [Rexus QZ30 product specification](https://rexus.id/products/rexus-flow-qz30-mouse-office-wireless-ergonomic-silent-click-aerospin-wheel-horizontal-scroll-mode): Pixart 3220 optical sensor; preset DPI 1000, 1600, 2400, 3000, 4800; Bluetooth 5.0 and 2.4 GHz; 500 mAh battery. The sensor designation does not identify the controller MCU.
- [Official downloads](https://rexus.id/pages/driver): Windows and Mac QZ Flow software explicitly includes QZ30 Black. No QZ30 firmware updater was listed in the firmware section at the time checked.

## Search outcome and unverified leads

Queries covered QZ30 firmware, GitHub, teardown/disassembly, PCB/MCU, Indonesian bongkar, receiver 276d:1902, and Loyal/1051 model names. No verified QZ30 firmware image, firmware source, MCU identification, or matching teardown was found. Search absence does not establish that none exists.

The local driver's `am_loyal_sendCommand` name led to a possible Shenzhen Loyal Electronics connection. [Loyal's product catalog](https://loyalelec.com/) includes ergonomic mice with related features, but neither appearance nor shared features establishes a board match. Some catalog pages were unavailable to the web fetcher.

Public FCC exhibits exist for [Loyal D1367C10](https://fccid.io/2AAVD-D1367C10) and [D1097E7](https://electric.garden/shenzhen-loyal-2aavd/wireless-mouse-d1097e7). Neither model was verified as the QZ30. Do not use their chip identities, firmware, or flashing instructions as QZ30-compatible evidence.

## Next evidence needed

First inspect the QZ30 underside label and receiver markings for an OEM model/FCC ID to narrow public filings. If those do not identify the board, clear PCB photographs and controller markings are needed. Firmware work additionally requires a verified readout/programming method and recovery path. Existing REA analysis of the host utility does not provide a verified QZ30 sensor-DPI write command; see DPI-NOTES.md.

## Software-only follow-up: D1051C8Z platform lead

The user does not want to open the mouse or contact Rexus. The next investigation uses public filings and Bluetooth service discovery instead.

- [Loyal D1051C8Z FCC filing](https://fccid.io/2AAVD-D1051C8Z), dated 2025-09-23, has public internal and external photos. Its model suffix aligns with the supplied Mac resource `15_1051/model.json`, whose RF model name is `1051(RF)` and receiver is 276d:1902.
- The [manufacturer-submitted manual](https://device.report/m/c1517e33376b0c0e4fb6f8fed60cc2b52adb5c4c6f245b479c0d907063c314dc) describes the matching distinctive control layout. Its listed presets are 1000/1600/2400/3200, differing from Rexus. This is strong evidence of a related OEM platform, not proof of identical firmware or board revision.
- Downloaded the [original internal-photo exhibit](https://fccid.io/2AAVD-D1051C8Z/Internal-Photos/Internal-Photos-8692192.pdf), verified SHA-256 `8bd82bebe94909b636d48ce754b92dbcb3c01db13edad57e500dce1fab91702c` against exhibit metadata, and extracted its original JPEG streams. Main board is pictured on PDF page 2 (`loyal-internal-page2-Image31.jpg`), with controller close-up in `Image32.jpg`. Visible board text includes `D1051C9-HS6621D_8034-MAIN-V03`. Controller marking is consistent with the 6621D family; exact suffix is not used as a compatibility guarantee.
- Downloaded [external-photo exhibit](https://fccid.io/2AAVD-D1051C8Z/External-Photos/External-Photos-8692191.pdf), verified SHA-256 `90f233c9d918aa8c6d699d774663bf98ccb8a93fe44eb8ad6bf3b922d9158c85`. Direct visual comparison with the app's `15_1051.png` matches the shell/control design. This still does not verify the actual QZ30 PCB.
- [OnMicro OM6621D specifications](https://www.onmicro.com.cn/enBLE/85.html) document Cortex-M4, serial flash, SWD and OTA support. [OnMicro boot/DFU explanation](https://bbs.onmicro.com.cn/BLEzhzl/112.html) covers HS6621x and OM6621x and describes application/backup partitions and boot metadata. Chip-family OTA capability does not establish that QZ30 firmware includes an accessible updater, readback, or unsigned-image support.
- The [OM6621E resource index](https://bbs.onmicro.com.cn/m/OM6621E/184.html) makes its SDK available through a business/technical contact after NDA. This concerns a related family, not proof of identical licensing or SDK availability for the older D variant. No usable QZ30 firmware or mouse SDK was obtained.

## Host-library firmware findings

REA/Ghidra decompiled the firmware info export, both upgrade overloads, and gaming DPI reload. Canonical Evidence is in `rea-dpi-{firmware-info,firmware-buffer,firmware-file,gaming-reload}.json`.

- Buffer upgrade `0x100156d0` requires a callback at device-record offset `0x65c`; file upgrade `0x100157c0` requires offset `0x658`. The selected type-17 initializer clears the record and does not explicitly populate either slot. No supported QZ30 flashing path was recovered from these exports. Helper side effects, unresolved dynamic paths and alternative firmware protocols remain outside this bounded conclusion.
- Gaming DPI reload `0x1000e040` accepts only types 3/4/5, excluding 17 before opening a device.
- Added hash-bound Capstone self-checks for both firmware callback offsets and their null branches to `inspect-dpi.py`; they passed. This is static inspection, not execution of the DLL against hardware.

## Bluetooth next step

On Andres, PnP receiver inventory reports 276d:1902, revision 0200, product `2.4G Wireless Receiver`; no mouse-specific paired Bluetooth entry was found by the bounded name/VID search. An 8-second BLE advertisement scan found no mouse-name candidates (`ble-advertisement-scan.json`). This does not prove there are no Bluetooth services; the mouse was not confirmed in pairing/advertising mode.

Prepared isolated Bleak dependencies at `~/.codex/tools/ble-inspect/` and a PDF parser at `~/.codex/tools/pdf-inspect/`; no system drivers or app dependencies changed. User input is pending to switch an unused mouse Bluetooth channel into pairing mode. Next: repeat scan, establish the correct device identity, enumerate GATT services and read standard model/firmware/manufacturer fields if exposed. Do not write DFU/control characteristics, trigger bootloader mode, or upload an image merely because an OTA UUID appears.

## Completed live Bluetooth check

The user enabled channel 2 pairing mode and confirmed ready. Advertisement scan identified `Rexus QZ30-2`; immediate connection after re-enabling pairing succeeded. Initial attempts timed out after advertising stopped; no setting or firmware change was needed. Connections used Bleak with `pair=False`, automatic disconnect, no notification subscriptions, and no application characteristic writes.

Read from the actual mouse's standard Device Information service:

| Field | Value |
| --- | --- |
| Manufacturer | `HomerTech` |
| Model | `D1063C_000F_01` |
| Firmware | `D1063C_v4.0.0` |
| Software | `SVN3663` |
| PnP ID | USB vendor source; VID `32c2`, PID `0012`, revision `0100` |

These live strings supersede assumptions that the shell's `1051` model number must also be its firmware model. They do not establish physical chip identity; manufacturers can reuse device-information strings.

Exposed services included standard GAP/GATT, Device Information, Battery, a DFU-style `FE59` service and vendor service `6e40ff01-b5a3-f393-e0a9-e50e24dcca9e`. The HID service `1812` was in advertising but absent from this Windows GATT inventory, so inventory completeness is not claimed.

The FE59 service contains:

- `8ec90001-f315-4f60-9fb8-838830daea50`: notify/write.
- `8ec90002-f315-4f60-9fb8-838830daea50`: write without response.
- `00000003-0000-1000-8000-00805f9b34fb`: read/write/notify. A plain ATT read returned ASCII `Version1.0`.

All three UUIDs and corresponding permissions match the `service_onmicro_dfu_init` example in the manufacturer's [OM6626 FAQ](https://bbs.onmicro.com.cn/m/OM6626/355.html), lines 60-85 when checked. This is a specific OnMicro protocol-family lead, not proof of an OM6626 chip or Nordic hardware. The Nordic-assigned FE59 UUID alone must not be used to identify the MCU or assume standard Nordic command semantics.

Vendor characteristics `6e40ff02...` (read/write without response) and `6e40ff03...` (read/notify) each returned twenty zero bytes on plain reads. No vendor command meaning or DPI control was established. Bounded local ASCII-string searches of the supplied Mac main executable and AMLIB did not find these UUIDs, `D1063`, `HomerTech` or DFU strings; this does not rule out binary UUID constants or dynamically loaded implementations.

Evidence: `ble-qz30-services.json`, `ble-qz30-vendor-reads.json`; digests, decoded PnP fields and limits in `ble-evidence-summary.json`. Assert checks verified PnP byte order, observed model/firmware, manufacturer-example UUID byte order, control-characteristic permissions and the `Version1.0` read. No serial-number characteristic was read. No firmware image was retrieved, signed-package policy tested, DFU command sent, bootloader triggered, or sensor DPI written.

Next research target is the OnMicro-compatible DFU/tool implementation and D1063C firmware/configuration format. A compatible image or verified readback path, image validation/signing policy, and recovery behavior remain prerequisites to rebuilding/flashing. The currently exposed vendor service may offer another configuration route, but its command protocol has not been recovered; do not guess writes from the presence of a writable characteristic.

## Vendor configuration investigation

Continued without further mouse interactions or device writes. Questions: whether the vendor service is a known DPI protocol; whether a supplied executable uses it; whether a public compatible driver supplies its commands.

- [OnMicro's central/peripheral example](https://bbs.onmicro.com.cn/m/OM6626/424.html) describes a transparent data service, with FF02 for host writes and FF03 notifications. Saved the original screenshot as `onmicro-tspp-example.png` and inspected it directly. It displays **16-bit** FF01/FF02/FF03 UUIDs, whereas this mouse exposes **128-bit** UUIDs beginning `6e40ff01/02/03`. Thus role/name similarity is only a lead, not an exact service or application-command match. This example does not establish a DPI command.
- [Related OnMicro TSPP example](https://bbs.onmicro.com.cn/OM6626/443.html) publishes attribute-table snippets, with the complete source attachment linked through login. The visible snippets concern descriptor/read behavior, not mouse DPI configuration. No full attachment was obtained.
- Inspected [Loyal's official download page](https://www.loyalelec.com/download), including its raw HTML file-download attributes. Listed files were a product catalog and SMETA/ISO/GRS/FSC/BSCI certificates, not a mouse SDK or utility. Saved the fetched HTML in `loyal-download-page.html`. This is a bounded page inventory, not a claim about all nonpublic OEM resources.
- Scanned the supplied Mac executable, AMLIB and Mouse Assistant for complete vendor UUIDs in lowercase/uppercase ASCII/UTF-16, RFC byte order, mixed-endian GUID order and reversed BLE byte order. No matches were found. Mac printable symbols include `IOHIDDeviceGetReport`, `IOHIDDeviceSetReport` and input-report callbacks, but no inspected printable CoreBluetooth transport names. Details and all three SHA-256 digests are in `vendor-service-static.json`. Dynamically built UUIDs, encrypted payloads and indirect/dynamic code remain outside this check.
- Read the public [OpenMouse protocol repository](https://github.com/OpenMouse-Project/mouse-protocol) main-branch archive without running any code. A bounded scan of 491 selected source/document files found no textual `32c2`, `276d`, `6e40ff`, `D1063`, `HomerTech`, `QZ30` or whole-word `Loyal` matches. This does not exhaust decimal or dynamically generated device IDs. Archive digest and search scope are in `public-protocol-search.json`.
- Also checked [the Acer PopGo companion source](https://github.com/ksanjeev284/acer-popgo-companion), which documents another OnMicro mouse. Its IDs are 32c2:0066/0026, and its reported BLE service set differs. Its negative DPI findings must not be generalized to QZ30; it yielded no matching vendor-service implementation in the bounded scan. GitHub repository-search API requests were rate-limited, so broad API coverage is not claimed.

Result: vendor-service **transport is partially characterized**, but its application protocol and any DPI command remain **unresolved**. There is no verified command to implement or test as a sensor-DPI write. The UUIDs and writable permissions alone cannot justify a setting probe, and the zero-filled reads are not evidence that DPI is or is not programmable. No live BLE connection, subscriptions, writes, firmware upload, app modification or new physical preset test was performed during this pass.

Remaining evidence needed is a QZ30/D1063C-compatible configuration implementation or actual firmware image/readback with recoverable command handlers. Generic OnMicro DFU tooling might clarify image format and update behavior, but it cannot by itself reveal mouse-specific DPI commands. Custom sensor-DPI editing remains unimplemented; no software-only success is promised from the present artifacts.

## Firmware package/readback investigation

Downloaded the manufacturer-authored [OM6621E SDK User Guide](https://docs.gtai-tech.com/static-file/eb200d4b-e47d-45c1-b1a5-562700c54cd7/fbc01b51-c130-4eec-a6fe-b414f7c2110c.pdf), 50 pages, to `onmicro-sdk-guide.pdf`. Its identity and reviewed page scope are in `onmicro-sdk-guide-evidence.json`; page-count and expected tool/signature terminology assertions passed. This is related chip-family documentation, not proof of the QZ30 MCU or firmware policy.

- Pages 22-24 document OTA transmission into an on-device backup/staging area, verification, restart and copying by the bootloader. This **backup area is not a firmware-export/backup feature**. No Bluetooth firmware readback procedure was recovered in the reviewed OTA section.
- `CreateFwTool` packs already-compiled images; it does not recover original firmware. Its documented options include image types, versions, raw data/address fields and private-key signatures. This establishes that signed packages are possible in the family, not that the QZ30 requires or accepts them. There is no QZ30-compatible image, key or address map in this guide.
- The [manufacturer's signed-OTA note](https://bbs.onmicro.com.cn/6621CxB_6621CxC/249.html) describes project-specific signature checking, but its full attachment is login-linked. QZ30's actual validation policy remains untested.
- The [manufacturer's dump-tool instructions](https://bbs.onmicro.com.cn/OM6621E/182.html) require a connected Segger J-Link/GDB server and mention serial flash readout. This is a hardware-debugging procedure, not a verified no-opening Bluetooth backup route. No debug/protection/unlock procedure was attempted.
- The public [OMToolbox download answer](https://bbs.onmicro.com.cn/6621CxB_6621CxC/242.html) links `www.onmicroapp.com.cn/Onmicro/public/index.php/download_firmware?url=OMToolbox-release.apk`. That host and its no-www variant failed DNS resolution on Andres. The web fetcher also could not access the link. An alternate `.com` host did not resolve either; it is not treated as an authenticated alternate source. A bounded archive.org index request timed out. No APK was obtained, installed or executed; no new Android decompiler/provider installation was necessary.
- [OM6626 boot/OTA example](https://bbs.onmicro.com.cn/OM6626/523.html) and [DFU guide](https://bbs.onmicro.com.cn/OM6626/576.html) expose useful explanatory text, but clicking their source/archive attachments redirects to the forum login page. No account was created and no protected attachment was obtained. These examples are not known QZ30 firmware.
- Searched for the guide's package-header example bytes `55 df bf 01` in the supplied AMLIB, Mouse Assistant, Mac main executable, Assets.car, and the two original installer ZIP files. No matches; identities and offsets are recorded in `onmicro-package-signature-search.json`. ZIP files were scanned as supplied bytes, not exhaustively decompressed in this step. This does not exclude raw, compressed, encrypted or differently packaged firmware.

Status: package format is partially documented; QZ30 firmware image, Bluetooth readback, validation policy and recovery remain unresolved. Public-source and supplied-artifact paths checked in this investigation have not yielded a working no-opening method for sensor-DPI editing or firmware rebuilding. Further work needs new evidence (a compatible image/configuration implementation, or accessible model-relevant protocol/source), not repeated generic updater calls. No device command, firmware write, app modification or firmware rebuild was performed.

## Final workaround check

At the user's request for one last check, repeated focused public searches for QZ30 custom DPI, live model/firmware strings `D1063C_000F_01` / `D1063C_v4.0.0`, receiver 276d:1902, related D1051C8Z software and OMToolbox availability. No verified compatible DPI command, firmware image or Bluetooth backup procedure emerged. Searches returning manuals for a related mouse, or findings for a different OnMicro PID, do not establish QZ30 configuration support.

There is a movement-speed workaround, explicitly distinct from editable sensor DPI:

- [Raw Accel](https://github.com/RawAccelOfficial/rawaccel) is a Windows 10/11 x86-64 driver that transforms the raw mouse-input stream. Its [official guide](https://github.com/RawAccelOfficial/rawaccel/blob/master/doc/Guide.md) documents the sensitivity multiplier as scaling the output vector; installation requires a driver and restart. A fixed multiplier can approximate a chosen movement sensitivity while the hardware stays on one known preset. It does not rewrite the sensor, increase captured physical resolution, or replace the onboard stage table.
- [LinearMouse](https://linearmouse.app/en/) on macOS documents pointer-speed/acceleration adjustment and device/app/display profiles. This changes host pointer behavior, not firmware DPI. Equivalent settings and behavior between the Windows raw-input driver and Mac pointer pipeline are not assumed; physical calibration and application testing remain needed.

Recommendation: if the user accepts host-side movement sensitivity as a fallback, use clearly labeled pointer-sensitivity profiles with the mouse left on a known hardware preset. Do not present those profiles as sensor-DPI editing. Neither tool was installed or tested with this QZ30, no settings were changed, and the app was not modified in this check. Actual onboard DPI editing remains unresolved under the no-opening constraint.
