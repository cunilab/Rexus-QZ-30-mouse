# Action behavior

The editor keeps changes as a draft until **Save and apply**. Existing mappings continue during editing. Custom shortcuts use distinct Control, Shift, Alt/Option, and Windows/Command modifiers plus one key. Key names follow the US physical-key layout; other layouts need testing.

| Action | Windows | macOS |
| --- | --- | --- |
| Previous / next desktop | Ctrl+Win+Left / Right | Ctrl+Left / Right |
| Task View / Mission Control | Win+Tab | Ctrl+Up |
| Show desktop | Win+D | F11 (configured Mission Control shortcut) |
| Switch apps | Alt+Tab | Command+Tab |
| Minimize | Native foreground-window minimize | Command+M |
| Maximize / restore | Native foreground-window toggle | Toggle full screen: Control+Command+F |
| Previous / next tab | Ctrl+Shift+Tab / Ctrl+Tab | Control+Shift+Tab / Control+Tab |
| New / close / reopen tab | Ctrl+T / Ctrl+W / Ctrl+Shift+T | Command+T / Command+W / Command+Shift+T |
| Refresh | Ctrl+R | Command+R |
| Editing | Ctrl-based shortcuts | Command-based shortcuts; Redo adds Shift |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 | Command+= / Command+- / Command+0 |
| Screenshot | Win+Shift+S | Command+Shift+3 |
| Media | Native Windows media keys | Native AppKit media events |

Browser/tab/zoom commands follow the foreground application's shortcuts. Window commands target the foreground window. Mac full screen and Windows maximize have different behavior and labels. Desktop switching needs multiple desktops/Spaces and enabled OS shortcuts. Check the machine's Mission Control settings and each target application's shortcuts before relying on them. Standard combinations are referenced in [Microsoft's shortcut guide](https://support.microsoft.com/en-au/windows/keyboard-shortcuts-in-windows-dcc61a57-8ff0-cffe-9796-cb9706c75eec); Mac media identifiers follow [Apple's published HID definitions](https://github.com/apple-oss-distributions/IOHIDFamily/blob/main/IOHIDSystem/IOKit/hidsystem/ev_keymap.h).

## Record a shortcut

Choose **Custom keyboard shortcut**, then **Record shortcut**. Keep QZ Flow focused, release held keys, press the chord, and release it. Escape, loss of focus, or ten seconds cancels recording. Captured releases are drained before the recorder closes, with a bounded cleanup timeout. Recording temporarily bypasses mapped mouse actions. Injected events are not used for recording; OS-reserved and secure shortcuts may bypass native hooks. Use the manual key/modifier controls for reserved chords. macOS requires Accessibility and Input Monitoring; recording never requests permission in the background.

Recording produces a draft, and does not execute that shortcut as a test. Do not use secure OS sequences such as Ctrl+Alt+Delete or lock-screen chords as a recording test. Actual native capture and non-US layouts still need hardware verification.

## Launch an application

Choose **Launch application**, then **Choose application**. Windows accepts an existing absolute `.exe` path; Mac accepts a `.app` bundle. Arguments, scripts, and shell commands are not supported. The selected path is persisted only after Save. Launching is dispatched outside the input listener, with failures shown in status. An application that is removed or moved must be selected again.

## Disable and verification

**Default behavior** passes native input through. **Disable button** consumes OS-hook events. It is offered for Middle, Back, and Forward on Windows; raw receiver wheel/thumb input cannot be reliably suppressed. Mac supports OS mouse/scroll events but has no verified thumb-rest support.

Verify one assignment at a time with the original mouse utility closed: desktop switching with two desktops, a tab/editing action in a disposable document, media controls, and launch of a known application. Confirm one action per press, paired key release, Pause/Resume, disconnect/reconnect, and Quit. Native dialogs, recording, and physical Windows/Mac execution are separate from unit/build checks; they are not claimed as verified by compilation.
